mod args;
mod db;
mod images;
mod thresh_acc;

use args::parse_args;
use db::{
    add_to_table, get_images_to_archive, populate_new_table, set_images_as_archived,
    update_table_get_new, TableType::*,
};
use images::{archive_image, load_images, ImageAdv, ImageBasic};
use indicatif::{ParallelProgressIterator, ProgressStyle};
use log::{info, warn, LevelFilter};
use rayon::iter::{IntoParallelIterator, ParallelIterator};

fn get_prog_style() -> ProgressStyle {
    ProgressStyle::with_template("{msg} [{elapsed} / {duration}] {wide_bar} {pos} / {len}")
        .expect("Illegal Progress Bar Template")
}

fn main() -> anyhow::Result<()> {
    env_logger::builder()
        .filter_level(LevelFilter::Warn)
        .format_timestamp(None)
        .format_target(false)
        .parse_env("RAWDB_LOG")
        .init();
    let args = parse_args()?;

    eprintln!("Loading database at {}", args.database_path.display());
    let mut conn = db::create_conn(&args.database_path, args.clean)?;

    // Read file structure on disk, find rows that don't exist in in on_disk
    // An unknown file in the target is an error
    eprintln!("Scanning target at {}", args.target_dir.display());
    let target_images =
        load_images::<ImageBasic>(&args.target_dir).collect::<Result<Vec<_>, _>>()?;
    info!("  Found {} target images", target_images.len());

    {
        let trans = conn.transaction()?;
        populate_new_table(&trans, Disk, &target_images)?;
        let new_on_disk = update_table_get_new(&trans, Disk)?;

        // For those new rows, read their metadata by actually opening the files
        let new_on_disk_adv = new_on_disk
            .into_par_iter()
            .progress_with_style(get_prog_style())
            .with_message("Indexing new disk images")
            .filter_map(|i| {
                ImageAdv::from_basic(i)
                    .inspect_err(|err| warn!("{}", err))
                    .ok()
            })
            .collect::<Vec<_>>();

        // With that new metadata, add the rows to the database
        add_to_table(&trans, Disk, &new_on_disk_adv)?;
        trans.commit()?;
    };

    let Some(source_dir) = args.source_dir else {
        return Ok(());
    };

    // Read the file structure on the camera, find the rows that don't exist in on_camera
    eprintln!("Finding images in {:?}", source_dir);
    let images = load_images::<ImageBasic>(&source_dir)
        .filter_map(|res| res.inspect_err(|err| warn!("{}", err)).ok())
        .collect::<Vec<_>>();
    info!("  Found {} images", images.len());

    {
        let trans = conn.transaction()?;
        populate_new_table(&trans, Camera, &images)?;
        let new_on_camera = update_table_get_new(&trans, Camera)?;

        // For those new rows, read their metadata by actually opening the files
        let new_on_camera_adv = new_on_camera
            .into_par_iter()
            .progress_with_style(get_prog_style())
            .with_message("Indexing new camera images")
            .filter_map(|i| {
                ImageAdv::from_basic(i)
                    .inspect_err(|err| warn!("{}", err))
                    .ok()
            })
            .collect::<Vec<_>>();

        // With that new metadata, add the rows to the database
        add_to_table(&trans, Camera, &new_on_camera_adv)?;

        trans.commit()?;
    }

    let images_to_archive = get_images_to_archive(&conn)?;

    {
        let trans = conn.transaction()?;
        let success = images_to_archive
            .into_par_iter()
            .progress_with_style(get_prog_style())
            .with_message("Archiving images")
            .filter_map(|image| {
                archive_image(&image, &args.target_dir)
                    .inspect_err(|err| warn!("{}", err))
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
