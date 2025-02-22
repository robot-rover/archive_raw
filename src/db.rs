use std::path::Path;

use blake3::Hash;
use rusqlite::{config::DbConfig, params, Connection};

use crate::images::{ImageAdv, ImageBasic};

const APPLICATION_ID: i64 = 0xBEEF;
const USER_VERSION: i64 = 2;

#[derive(Debug)]
pub enum TableType {
    Disk,
    Camera,
}

impl TableType {
    fn to_sql(&self, is_new: bool) -> &str {
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
    let conn= Connection::open(db_file)?;

    let application_id: i64 = conn.pragma_query_value(
        None,
        "application_id",
        |row| row.get(0),
    )?;

    if clean || application_id != APPLICATION_ID {
        // TODO: Perhaps ask before doing this?
        println!("application_id is unset, resetting database");
        // Reset the database
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_RESET_DATABASE, true)?;
        conn.execute("VACUUM", [])?;
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_RESET_DATABASE, false)?;

        conn.pragma_update(None, "application_id", APPLICATION_ID)?;
    }

    println!("Application ID: {}", application_id);

    let user_version: i64 = conn.pragma_query_value(
        None,
        "user_version",
        |row| row.get(0),
    )?;

    println!("User Version: {}", user_version);

    if user_version != USER_VERSION {
        println!("Updating schema from version {} to {}", user_version, USER_VERSION);
        update_schema(&conn, user_version)?;
        conn.pragma_update(None, "user_version", USER_VERSION)?;
    }

    Ok(conn)
}

fn update_schema(conn: &Connection, current_user_version: i64) -> anyhow::Result<()> {
    if current_user_version < 0 || current_user_version > USER_VERSION {
        anyhow::bail!("Unsupported user version: {} (Expected {})", current_user_version, USER_VERSION);
    }

    if current_user_version < 1 {
        conn.execute_batch(
            include_str!("schema/v1.sql"),
        )?;
    }

    Ok(())
}

pub fn populate_new_table<'a, I>(conn: &Connection, table: TableType, images: I) -> anyhow::Result<()>
where I: IntoIterator<Item=&'a ImageBasic> {
    let name = table.to_sql(true);
    conn.execute_batch(&format!("
        DROP TABLE IF EXISTS {name};
        CREATE TABLE {name} (
          name     TEXT NOT NULL,
          path     TEXT NOT NULL,
          size      INT NOT NULL
        ) STRICT;"))?;

    let mut stmt = conn.prepare(
        &format!("INSERT INTO {name} (name, path, size) VALUES (?1, ?2, ?3)")
    )?;

    for image in images {
        stmt.execute(params![&image.get_name(), &image.path, &image.size])?;
    }

    Ok(())
}

pub fn update_table_get_new(conn: &Connection, table: TableType) -> anyhow::Result<Vec<ImageBasic>> {
    let name = table.to_sql(false);
    let new_name = table.to_sql(true);

    let delete_count = conn.execute(&format!("
        DELETE FROM {name}
        WHERE rowid in (
            SELECT {name}.rowid
            FROM {name}
            LEFT JOIN {new_name}
            ON {name}.path = {new_name}.path
                AND {name}.size = {new_name}.size
            WHERE {new_name}.name IS NULL
        )
    "), [])?;

    println!("Deleting {} image entries that no longer exist", delete_count);

    let keep_count = conn.query_row(&format!("
        SELECT COUNT(*)
        FROM {name}
        INNER JOIN {new_name}
        ON {name}.path = {new_name}.path
            AND {name}.size = {new_name}.size
    "), [], |row| row.get::<_, u64>(0))?;
    println!("Keeping {} existing image entries", keep_count);

    let mut stmt = conn.prepare(&format!("
        SELECT {new_name}.path, {new_name}.size
        FROM {new_name}
        LEFT JOIN {name}
        ON {name}.path = {new_name}.path
            AND {name}.size = {new_name}.size
        WHERE {name}.name IS NULL
    "))?;

    let im_basic = stmt.query_map([], |row| Ok(ImageBasic {
            path: row.get(0)?,
            size: row.get(1)?,
    }))?.collect::<Result<Vec<_>, _>>()?;

    Ok(im_basic)
}

pub fn add_to_table<'a, I>(conn: &Connection, table: TableType, images: I) -> anyhow::Result<()>
where I: IntoIterator<Item=&'a ImageAdv> {
    let name = table.to_sql(false);
    let mut stmt = conn.prepare(&format!("
        INSERT INTO {name} (name, path, size, checksum, date)
        VALUES (?1, ?2, ?3, ?4, ?5)
    "))?;

    for image in images.into_iter() {
        stmt.execute(params![&image.basic.get_name(), &image.basic.path, &image.basic.size, &image.checksum.as_bytes(), &image.date])?;
    }

    Ok(())
}

pub fn get_images_to_archive(conn: &Connection) -> anyhow::Result<Vec<ImageAdv>> {
    let mut stmt = conn.prepare("
        SELECT on_camera.path, on_camera.size, on_camera.checksum, on_camera.date
        FROM on_camera
        LEFT JOIN on_disk
        ON on_disk.name = on_camera.name
            AND on_disk.size = on_camera.size
            AND on_disk.checksum = on_camera.checksum
            AND on_disk.date = on_camera.date
        WHERE on_disk.name IS NULL
            AND on_camera.saved = 0
    ")?;

    let images = stmt.query_map([], |row| Ok(ImageAdv {
        basic: ImageBasic {
            path: row.get(0)?,
            size: row.get(1)?,
        },
        checksum: Hash::from_bytes(row.get::<_, [u8; blake3::OUT_LEN]>(2)?),
        date: row.get(3)?,
    }))?.collect::<Result<Vec<_>, _>>()?;

    Ok(images)
}

pub fn set_images_as_archived<'a, I>(conn: &Connection, saved: I) -> anyhow::Result<()>
where I: IntoIterator<Item=&'a ImageAdv> {
    conn.execute_batch(include_str!("schema/saved.sql"))?;
    let mut stmt = conn.prepare("
        INSERT INTO make_saved (path)
        VALUES (?1)
    ")?;

    for image in saved.into_iter() {
        stmt.execute([&image.basic.path])?;
    }

    conn.execute("
        UPDATE on_camera
        SET saved = 1
        WHERE path in (SELECT path FROM make_saved)
    ", [])?;

    Ok(())
}
