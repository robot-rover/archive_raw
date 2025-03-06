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
    MultiProgress, ParallelProgressIterator, ProgressBar, ProgressIterator, ProgressStyle,
};
use indicatif_log_bridge::LogWrapper;
use log::{error, info, warn, LevelFilter};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
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
    populate_new_table(&trans, table, &target_images)?;
    let new_on = update_table_get_new(&trans, table)?;

    // For those new rows, read their metadata by actually opening the files
    pb.set_length(new_on.len() as u64);
    let new_on_adv = new_on
        .into_par_iter()
        .progress_with(pb)
        .with_message(format!("Indexing new {} images", table.label()))
        .filter_map(|i| {
            ImageAdv::from_basic(i)
                .inspect_err(|err| warn!("{}", err))
                .ok()
        })
        .collect::<Vec<_>>();

    // With that new metadata, add the rows to the database
    add_to_table(&trans, table, new_on_adv)?;
    trans.commit()?;

    Ok(())
}

fn find_new_files_multi(
    conn: &mut Connection,
    table: TableType,
    dir: &Path,
    label: &str,
    multi: &MultiProgress,
) -> anyhow::Result<()> {
    let pb = multi.add(ProgressBar::no_length().with_style(get_prog_style()));
    let res = find_new_files(conn, table, dir, label, pb.clone());
    pb.finish();
    multi.remove(&pb);
    res
}

fn main() -> anyhow::Result<()> {
    let logger_inner = env_logger::builder()
        .filter_level(LevelFilter::Warn)
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

    find_new_files_multi(&mut conn, Disk, &args.target_dir, "target", &multi)?;

    let Some(source_dir) = args.source_dir else {
        return Ok(());
    };

    find_new_files_multi(&mut conn, Camera, &source_dir, "source", &multi)?;

    let images_to_archive = get_images_to_archive(&conn)?;

    {
        let trans = conn.transaction()?;
        let success = images_to_archive
            .into_par_iter()
            .progress_with_style(get_prog_style())
            .with_message("Archiving images")
            .filter_map(|image| {
                archive_image(&image, &args.target_dir)
                    .inspect_err(|err| error!("{}", err))
                    .map(|_| image)
                    .ok()
            })
            .collect::<Vec<_>>();

        set_images_as_archived(&trans, success.iter())?;
        trans.commit()?;
        eprintln!("Archived {} images", success.len());
    }

    Ok(())
}
