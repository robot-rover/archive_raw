mod args;
mod db;
mod images;

use std::path::Path;

use args::parse_args;
use db::{
    add_to_table, get_images_to_archive, populate_new_table, set_images_as_archived,
    update_table_get_new,
    TableType::{self, *},
};
use images::{archive_image, load_images, ImageAdv, ImageBasic};
use indicatif::{
    MultiProgress, ProgressBar, ProgressIterator, ProgressStyle
};
use indicatif_log_bridge::LogWrapper;
use log::{error, info, warn, LevelFilter};
use rusqlite::Connection;

fn get_prog_style() -> ProgressStyle {
    ProgressStyle::with_template("{msg} [{elapsed} / {duration}] {wide_bar} {pos} / {len}")
        .expect("Illegal Progress Bar Template")
}

fn find_new_files(
    conn: &mut Connection,
    table: TableType,
    dir: &Path,
    label: &str,
    pb: ProgressBar,
) -> anyhow::Result<()> {
    // Read file structure on disk, find rows that don't exist in in on_disk
    // An unknown file in the target is an error
    eprintln!("Scanning {} at {}", label, dir.display());
    let target_images = load_images::<ImageBasic>(dir).collect::<Result<Vec<_>, _>>()?;
    info!("  Found {} {} images", target_images.len(), label);

    let trans = conn.transaction()?;
    let duplicates = populate_new_table(&trans, table, &target_images)?;
    for dup in duplicates {
        error!("Possible duplicate file detected: {}", dup.name);
        for path in dup.paths {
            error!("  {}", path);
        }
    }
    let new_on = update_table_get_new(&trans, table)?;

    // For those new rows, read their metadata by actually opening the files
    pb.set_length(new_on.len() as u64);
    let new_on_adv = new_on
        .into_iter()
        .progress_with(pb)
        .with_message(format!("Indexing new {} images", table.label()))
        .filter_map(|i| {
            ImageAdv::from_basic(i, dir)
                .inspect_err(|err| warn!("{}", err))
                .ok()
        })
        .collect::<Vec<_>>();

    // With that new metadata, add the rows to the database
    add_to_table(&trans, table, new_on_adv)?;
    trans.commit()?;

    Ok(())
}

fn wrap_multi<F, T>(
    multi: &MultiProgress,
    inner: F
) -> T
where F: FnOnce(ProgressBar) -> T {
    let pb = multi.add(ProgressBar::no_length().with_style(get_prog_style()));
    let res = inner(pb.clone());
    pb.finish();
    multi.remove(&pb);
    res
}

fn main() -> anyhow::Result<()> {
    let logger_inner = env_logger::builder()
        .filter_level(LevelFilter::Info)
        .format_timestamp(None)
        .format_target(false)
        .parse_env("RAWDB_LOG")
        .build();

    let multi = MultiProgress::new();
    LogWrapper::new(multi.clone(), logger_inner)
        .try_init()
        .expect("Failed to initialize logger");

    let args = parse_args()?;

    eprintln!("Loading database at {}", args.database_path.display());
    let mut conn = db::create_conn(&args.database_path, args.clean)?;

    if args.clean {
        eprintln!("Database cleaned, exiting...");
        return Ok(())
    }

    wrap_multi(&multi, |pb| find_new_files(&mut conn, Disk, &args.target_dir, "target", pb))?;

    let Some(source_dir) = args.source_dir else {
        return Ok(());
    };

    wrap_multi(&multi, |pb| find_new_files(&mut conn, Camera, &source_dir, "source", pb))?;

    let images_to_archive = get_images_to_archive(&conn)?;

    if args.dry {
        println!("Images to archive:");
        for image in &images_to_archive {
            println!("  {}", image.basic.path);
        }

        return Ok(())
    }
    wrap_multi(&multi, |pb| {
        pb.set_length(images_to_archive.len() as u64);

        let trans = conn.transaction()?;
        let success = images_to_archive
            .into_iter()
            .progress_with(pb)
            .with_message("Archiving images")
            .filter_map(|image| {
                archive_image(&image, &source_dir, &args.target_dir)
                    .inspect_err(|err| error!("{}", err))
                    .map(|_| image)
                    .ok()
            })
            .collect::<Vec<_>>();

        set_images_as_archived(&trans, success.iter())?;
        trans.commit()?;
        eprintln!("Archived {} images", success.len());

        Ok(())
    })
}
