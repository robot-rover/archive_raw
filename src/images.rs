use anyhow::{anyhow, bail, Context};
use blake3::Hash;
use std::{ffi::OsStr, fs, path::Path};

use chrono::NaiveDateTime;
use rexiv2::Metadata;
use walkdir::{DirEntry, WalkDir};

pub trait ImageExt: Sized {
    fn from_entry(entry: &DirEntry) -> anyhow::Result<Self>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImageBasic {
    pub path: String,
    pub size: u64,
}

impl ImageExt for ImageBasic {
    fn from_entry(entry: &DirEntry) -> anyhow::Result<Self> {
        let path = entry
            .path()
            .to_str()
            .ok_or_else(|| anyhow!("Path {} is not utf8", entry.path().display()))?
            .to_owned();

        let size = entry.metadata()?.len();
        Ok(ImageBasic { path, size })
    }
}

impl ImageBasic {
    pub fn get_name(&self) -> &str {
        AsRef::<Path>::as_ref(&self.path)
            .file_name()
            .and_then(OsStr::to_str)
            .expect("Convertion from str to path and back failed")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImageAdv {
    pub basic: ImageBasic,
    pub checksum: Hash,
    pub date: NaiveDateTime,
}

impl ImageAdv {
    pub fn from_basic(basic: ImageBasic) -> anyhow::Result<Self> {
        let content = fs::read(&basic.path)
            .with_context(|| format!("Unable to read content of {}", basic.path))?;

        let metadata = Metadata::new_from_buffer(&content)
            .with_context(|| format!("Unrecognized image format in {}", basic.path))?;

        if !metadata.has_exif() {
            bail!("No exif data found in {}", basic.path);
        }

        let date_str = metadata
            .get_tag_string("Exif.Image.DateTime")
            .with_context(|| format!("No exif date found in {}", basic.path))?;

        let date = NaiveDateTime::parse_from_str(&date_str, "%Y:%m:%d %H:%M:%S")
            .with_context(|| format!("Unable to parse exif date in {}", basic.path))?;

        let checksum = blake3::hash(&content);

        Ok(ImageAdv {
            basic,
            checksum,
            date,
        })
    }
}

impl ImageExt for ImageAdv {
    fn from_entry(entry: &DirEntry) -> anyhow::Result<Self> {
        ImageAdv::from_basic(ImageBasic::from_entry(entry)?)
    }
}

const IGNORE_EXT: &[&str] = &["xmp", "pp3"];

pub fn load_images<I: ImageExt>(dir: &Path) -> impl Iterator<Item = anyhow::Result<I>> {
    WalkDir::new(dir)
        .into_iter()
        .map(|res| match res {
            Ok(entry) if entry.file_type().is_file() => {
                let ext = AsRef::<Path>::as_ref(entry.file_name())
                    .extension()
                    .and_then(OsStr::to_str);
                match ext {
                    Some(ext) if IGNORE_EXT.contains(&ext) => Ok(None),
                    _ => Ok(Some(I::from_entry(&entry)?)),
                }
            }
            Ok(_dir_entry) => Ok(None),
            Err(err) => Err(err.into()),
        })
        .filter_map(Result::transpose)
}

pub fn archive_image(image: &ImageAdv, target_base: &Path) -> anyhow::Result<()> {
    let mut target = target_base.join(image.date.format("%Y-%m-%d").to_string());
    fs::create_dir_all(&target)
        .with_context(|| format!("Failed to create directory {}", target.display()))?;

    target.push(image.basic.get_name());

    if fs::exists(&target)? {
        bail!("File {} already exists", target.display());
    }

    fs::copy(&image.basic.path, &target).with_context(|| {
        format!(
            "Failed to copy to {} to {}",
            &image.basic.path,
            target.display()
        )
    })?;

    let new_hash = blake3::hash(
        &fs::read(&target).with_context(|| format!("Failed to re-read {}", target.display()))?,
    );

    if new_hash != image.checksum {
        fs::remove_file(&target)?;
        bail!("Checksum mismatch for {}", target.display());
    }

    Ok(())
}
