use std::path::Path;

use anyhow::bail;
use anyhow::Context;
use log::debug;
use log::error;
use log::info;
use rusqlite::{config::DbConfig, params, Connection};

use crate::images::{ImageAdv, ImageBasic};

const APPLICATION_ID: i64 = 0xBEEF;
const USER_VERSION: i64 = 2;

#[derive(Copy, Clone, Debug)]
pub enum TableType {
    Disk,
    Camera,
}

impl TableType {
    pub fn label(&self) -> &str {
        match self {
            TableType::Disk => "disk",
            TableType::Camera => "camera",
        }
    }

    fn to_sql(self, is_new: bool) -> &'static str {
        match (self, is_new) {
            (TableType::Disk, false) => "on_disk",
            (TableType::Disk, true) => "new_on_disk",
            (TableType::Camera, false) => "on_camera",
            (TableType::Camera, true) => "new_on_camera",
        }
    }
}

// TODO: Function that validates paths / names match up

pub fn create_conn(db_file: &Path, clean: bool) -> anyhow::Result<Connection> {
    let conn = Connection::open(db_file).context("Unable to open database file")?;

    let application_id: i64 = conn.pragma_query_value(None, "application_id", |row| row.get(0))?;

    if clean || application_id != APPLICATION_ID {
        // TODO: Perhaps ask before doing this?
        debug!("application_id is unset, resetting database");
        // Reset the database
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_RESET_DATABASE, true)?;
        conn.execute("VACUUM", [])?;
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_RESET_DATABASE, false)?;

        conn.pragma_update(None, "application_id", APPLICATION_ID)?;
    }

    let user_version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

    if user_version != USER_VERSION {
        debug!(
            "Updating schema from version {} to {}",
            user_version, USER_VERSION
        );
        update_schema(&conn, user_version)?;
        conn.pragma_update(None, "user_version", USER_VERSION)?;
    }

    Ok(conn)
}

fn update_schema(conn: &Connection, current_user_version: i64) -> anyhow::Result<()> {
    if !(0..=USER_VERSION).contains(&current_user_version) {
        anyhow::bail!(
            "Unsupported user version: {} (Expected {})",
            current_user_version,
            USER_VERSION
        );
    }

    if current_user_version < 1 {
        conn.execute_batch(include_str!("schema/v1.sql"))?;
    }

    Ok(())
}

pub struct DuplicateImage {
    pub name: String,
    pub paths: Vec<String>,
}

pub fn populate_new_table<'a, I>(
    conn: &Connection,
    table: TableType,
    images: I,
    leave: bool,
) -> anyhow::Result<Vec<DuplicateImage>>
where
    I: IntoIterator<Item = &'a ImageBasic>,
{
    let name = table.to_sql(true);
    conn.execute_batch(&format!(
        "
        DROP TABLE IF EXISTS {name};
        CREATE {} TABLE {name} (
          name     TEXT NOT NULL,
          path     TEXT NOT NULL,
          size      INT NOT NULL
        ) STRICT;

        CREATE UNIQUE INDEX {name}_path
        ON {name}(path);

        CREATE INDEX {name}_uniq
        ON {name}(name, size);
    ",
        if leave { "" } else { "TEMP" },
    ))?;

    let mut stmt = conn.prepare(&format!(
        "INSERT INTO {name} (name, path, size) VALUES (?1, ?2, ?3)"
    ))?;

    for image in images {
        stmt.execute(params![&image.get_name(), &image.path, &image.size])?;
    }

    let duplicates = conn
        .prepare(&format!(
            "
        SELECT name, size
        FROM {name}
        GROUP BY name, size
        HAVING COUNT(*) > 1
    "
        ))?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<Vec<(String, i64)>, _>>()?;

    if duplicates.is_empty() {
        return Ok(Vec::new());
    }

    let mut dup_stmt = conn.prepare(&format!(
        "
        SELECT path
        FROM {name}
        WHERE name = ?1 AND size = ?2
    "
    ))?;

    let res = duplicates
        .into_iter()
        .map(|(name, size)| {
            let paths = dup_stmt
                .query_and_then(params![name, size], |row| row.get(0))?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(DuplicateImage { name, paths })
        })
        .collect::<Result<Vec<_>, anyhow::Error>>()?;

    conn.execute(
        &format!(
            "
        DELETE FROM {name}
        where rowid not in (
            SELECT rowid
            FROM {name}
            GROUP BY name, size
        )
    "
        ),
        [],
    )?;

    Ok(res)
}

pub fn update_table_get_new(
    conn: &Connection,
    table: TableType,
) -> anyhow::Result<Vec<ImageBasic>> {
    let name = table.to_sql(false);
    let new_name = table.to_sql(true);

    let delete_count = conn.execute(
        &format!(
            "
        DELETE FROM {name}
        WHERE rowid in (
            SELECT {name}.rowid
            FROM {name}
            LEFT JOIN {new_name}
            ON {name}.path = {new_name}.path
                AND {name}.size = {new_name}.size
            WHERE {new_name}.name IS NULL
        )
    "
        ),
        [],
    )?;
    info!(
        "{name} - Deleting {} image entries that no longer exist",
        delete_count
    );

    let keep_count = conn.query_row(&format!("SELECT COUNT(*) FROM {name}"), [], |row| {
        row.get::<_, u64>(0)
    })?;
    info!("{name} - Keeping {} existing image entries", keep_count);

    let mut stmt = conn.prepare(&format!(
        "
        SELECT {new_name}.path, {new_name}.size
        FROM {new_name}
        LEFT JOIN {name}
        ON {name}.path = {new_name}.path
            AND {name}.size = {new_name}.size
        WHERE {name}.name IS NULL
    "
    ))?;

    let im_basic = stmt
        .query_map([], |row| {
            Ok(ImageBasic {
                path: row.get(0)?,
                size: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    info!("{name} - detected {} new images", im_basic.len());

    Ok(im_basic)
}

pub fn add_to_table<'a, I>(conn: &Connection, table: TableType, images: I) -> anyhow::Result<()>
where
    I: IntoIterator<Item = ImageAdv>,
{
    let name = table.to_sql(false);
    let mut stmt = conn.prepare(&format!(
        "
        INSERT INTO {name} (name, path, size, date)
        VALUES (?1, ?2, ?3, ?4)
    "
    ))?;

    for image in images.into_iter() {
        debug!("Adding {} to {}", image.basic.path, table.to_sql(false));
        stmt.execute(params![
            &image.basic.get_name(),
            &image.basic.path,
            &image.basic.size,
            &image.date
        ])?;
    }

    Ok(())
}

pub fn get_images_to_archive(conn: &Connection) -> anyhow::Result<Vec<ImageAdv>> {
    let mut stmt = conn.prepare(
        "
        SELECT on_camera.path, on_camera.size, on_disk.path, on_disk.size
        FROM on_camera
        INNER JOIN on_disk
        ON on_disk.name = on_camera.name
            AND on_disk.date = on_camera.date
            AND on_disk.size != on_camera.size
    ",
    )?;

    let mismatch = stmt
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<Result<Vec<(String, i64, String, i64)>, _>>()?;

    if !mismatch.is_empty() {
        for (camera_path, camera_size, disk_path, disk_size) in mismatch {
            error!(
                "Image has size mismatch: Camera: {}={} Disk: {}={}",
                camera_path, camera_size, disk_path, disk_size
            );
        }
        bail!("Images with size mismatch detected");
    }

    let mut stmt = conn.prepare(
        "
        SELECT on_camera.path, on_camera.size, on_camera.date
        FROM on_camera
        LEFT JOIN on_disk
        ON on_disk.name = on_camera.name
            AND on_disk.date = on_camera.date
        WHERE on_disk.name IS NULL
            AND on_camera.saved = 0
    ",
    )?;

    let images = stmt
        .query_map([], |row| {
            Ok(ImageAdv {
                basic: ImageBasic {
                    path: row.get(0)?,
                    size: row.get(1)?,
                },
                date: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(images)
}

pub fn set_images_as_archived<'a, I>(conn: &Connection, saved: I) -> anyhow::Result<()>
where
    I: IntoIterator<Item = &'a ImageAdv>,
{
    conn.execute(
        "
        CREATE TEMP TABLE make_saved(
          path TEXT NOT NULL
        ) STRICT;
    ",
        [],
    )?;
    let mut stmt = conn.prepare(
        "
        INSERT INTO make_saved (path)
        VALUES (?1)
    ",
    )?;

    for image in saved.into_iter() {
        stmt.execute([&image.basic.path])?;
    }

    conn.execute(
        "
        UPDATE on_camera
        SET saved = 1
        WHERE path in (
            SELECT path
            FROM make_saved
        )
    ",
        [],
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use itertools::Itertools;
    use rand::prelude::*;

    const IN_MEMORY: &str = ":memory:";

    fn gen_random_image(counter: &mut u32) -> ImageAdv {
        let mut rng = rand::rng();
        *counter += 1;
        ImageAdv {
            basic: ImageBasic {
                path: format!("/path/{}.jpg", counter),
                size: rng.random::<u32>() as u64,
            },
            date: chrono::Utc::now().naive_utc(),
        }
    }

    fn gen_random_groups(enabled: Vec<bool>) -> Vec<Vec<ImageAdv>> {
        let mut vecs: Vec<Vec<ImageAdv>> = Vec::new();
        let mut rng = rand::rng();
        let mut image_counter = 0;

        for gen_images in enabled.into_iter() {
            let mut vec: Vec<ImageAdv> = Vec::new();
            if gen_images {
                vec.extend(
                    (0..rng.random_range(1..10)).map(|_| gen_random_image(&mut image_counter)),
                );
            }
            vecs.push(vec);
        }

        vecs
    }

    #[test]
    fn test_create_table() {
        let conn = create_conn(IN_MEMORY.as_ref(), false).unwrap();

        let app_id: i64 = conn
            .pragma_query_value(None, "application_id", |row| row.get(0))
            .unwrap();
        assert_eq!(
            app_id, APPLICATION_ID,
            "create_conn did not set the application_id correctly"
        );

        let user_version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(
            user_version, USER_VERSION,
            "create_conn did not set the user_version correctly"
        );
    }

    fn test_update_table(find_new: bool, find_common: bool, find_old: bool, table: TableType) {
        let vecs: Vec<Vec<ImageAdv>> = gen_random_groups(vec![find_new, find_common, find_old]);

        let conn = create_conn(IN_MEMORY.as_ref(), false).unwrap();

        // Setup tables
        populate_new_table(
            &conn,
            table,
            vecs[0].iter().chain(vecs[1].iter()).map(|i| &i.basic),
            false,
        )
        .unwrap();
        add_to_table(
            &conn,
            table,
            vecs[1].iter().cloned().chain(vecs[2].iter().cloned()),
        )
        .unwrap();

        let actual_new = update_table_get_new(&conn, table).unwrap();

        assert_eq!(
            vecs[0].iter().map(|i| &i.basic).collect::<Vec<_>>(),
            actual_new.iter().collect::<Vec<_>>(),
        );
    }

    #[test]
    fn test_update_table_loop() {
        for params in (0..4).map(|_| [false, true]).multi_cartesian_product() {
            let [new, common, old, is_camera] = params.try_into().unwrap();
            let table = if is_camera {
                TableType::Camera
            } else {
                TableType::Disk
            };
            println!(
                "new: {}, common: {}, old: {}, table: {:?}",
                new, common, old, table
            );
            test_update_table(new, common, old, table);
        }
    }

    fn test_archive_images(find_new: bool, find_common: bool, find_old: bool, set_archived: bool) {
        let vecs: Vec<Vec<ImageAdv>> = gen_random_groups(vec![find_new, find_common, find_old]);

        let conn = create_conn(IN_MEMORY.as_ref(), false).unwrap();

        // Setup tables
        add_to_table(
            &conn,
            TableType::Camera,
            vecs[0].iter().cloned().chain(vecs[1].iter().cloned()),
        )
        .unwrap();
        add_to_table(
            &conn,
            TableType::Disk,
            vecs[1].iter().cloned().chain(vecs[2].iter().cloned()),
        )
        .unwrap();
        if set_archived {
            set_images_as_archived(&conn, vecs[1].iter()).unwrap();
        }

        let actual_common = get_images_to_archive(&conn).unwrap();

        assert_eq!(
            vecs[0].iter().collect::<Vec<_>>(),
            actual_common.iter().collect::<Vec<_>>(),
        );
    }

    #[test]
    fn test_archive_images_loop() {
        for params in (0..4).map(|_| [false, true]).multi_cartesian_product() {
            let [new, common, old, set_archived] = params.try_into().unwrap();
            println!(
                "new: {}, common: {}, old: {}, set_archived: {}",
                new, common, old, set_archived
            );
            test_archive_images(new, common, old, set_archived);
        }
    }
}
