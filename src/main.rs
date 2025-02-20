mod args;
mod db;
mod images;

use args::parse_args;
use db::{add_to_table, populate_new_table, update_table_get_new, TableType::*};
use images::{load_images, ImageAdv, ImageBasic};

fn main() -> anyhow::Result<()> {
    let args = parse_args()?;

    println!("Loading database at {}", args.database_path.display());
    let mut conn = db::create_conn(&args.database_path, args.clean)?;

    // Read file structure on disk, find rows that don't exist in in on_disk
    println!("Scanning target at {}", args.target_dir.display());
    let target_images = load_images::<ImageBasic>(&args.target_dir)?;
    println!("Found {} target images", target_images.len());

    {
        let trans = conn.transaction()?;
        populate_new_table(&trans, Disk, &target_images)?;
        let new_on_disk = update_table_get_new(&trans, Disk)?;

        // For those new rows, read their metadata by actually opening the files
        let new_on_disk_adv = new_on_disk
            .into_iter()
            .map(|i| ImageAdv::from_basic(i))
            .collect::<Result<Vec<_>, _>>()?;

        // With that new metadata, add the rows to the database
        add_to_table(&trans, Disk, &new_on_disk_adv)?;
        trans.commit()?;
    };

    // Read the file structure on the camera, find the rows that don't exist in on_camera
    println!("Finding images in {:?}", args.source_dir);
    let images = load_images::<ImageBasic>(&args.source_dir)?;
    println!("Found {} images", images.len());

    {
        let trans = conn.transaction()?;
        populate_new_table(&trans, Camera, &images)?;
        let new_on_camera = update_table_get_new(&trans, Camera)?;

        // For those new rows, read their metadata by actually opening the files
        let new_on_camera_adv = new_on_camera
            .into_iter()
            .map(|i| ImageAdv::from_basic(i))
            .collect::<Result<Vec<_>, _>>()?;

        // With that new metadata, add the rows to the database
        add_to_table(&trans, Camera, &new_on_camera_adv)?;

        trans.commit()?;
    }

    Ok(())
}
