use anyhow::{anyhow, bail, Context};
use std::{ffi::OsStr, fs, path::Path};

use chrono::NaiveDateTime;
use rexiv2::Metadata;
use walkdir::{DirEntry, WalkDir};

pub trait ImageExt: Sized {
    fn from_entry(entry: &DirEntry, base: &Path) -> anyhow::Result<Self>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImageBasic {
    pub path: String,
    pub size: u64,
}

impl ImageExt for ImageBasic {
    fn from_entry(entry: &DirEntry, base: &Path) -> anyhow::Result<Self> {
        let path = entry
            .path()
            .strip_prefix(base)
            .context("Image path is not relative to base")?
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
    pub date: NaiveDateTime,
}

impl ImageAdv {
    pub fn from_basic(basic: ImageBasic, base: &Path) -> anyhow::Result<Self> {
        let abs_path = base.join(&basic.path);
        let metadata = Metadata::new_from_path(&abs_path)
            .with_context(|| format!("Unrecognized image format in {}", abs_path.display()))?;

        if !metadata.has_exif() {
            bail!("No exif data found in {}", abs_path.display());
        }

        let date_str = metadata
            .get_tag_string("Exif.Image.DateTime")
            .with_context(|| format!("No exif date found in {}", abs_path.display()))?;

        let date = NaiveDateTime::parse_from_str(&date_str, "%Y:%m:%d %H:%M:%S")
            .with_context(|| format!("Unable to parse exif date in {}", abs_path.display()))?;

        Ok(ImageAdv {
            basic,
            date,
        })
    }
}

impl ImageExt for ImageAdv {
    fn from_entry(entry: &DirEntry, base: &Path) -> anyhow::Result<Self> {
        ImageAdv::from_basic(ImageBasic::from_entry(entry, base)?, base)
    }
}

const IGNORE_EXT: &[&str] = &["xmp", "pp3"];

pub fn load_images<'a, I: ImageExt>(dir: &'a Path) -> impl Iterator<Item = anyhow::Result<I>> + use<'a, I> {
    WalkDir::new(dir)
        .into_iter()
        .map(|res| match res {
            Ok(entry) if entry.file_type().is_file() => {
                let ext = AsRef::<Path>::as_ref(entry.file_name())
                    .extension()
                    .and_then(OsStr::to_str);
                match ext {
                    Some(ext) if IGNORE_EXT.contains(&ext) => Ok(None),
                    _ => Ok(Some(I::from_entry(&entry, dir)?)),
                }
            }
            Ok(_dir_entry) => Ok(None),
            Err(err) => Err(err.into()),
        })
        .filter_map(Result::transpose)
}

pub fn archive_image(image: &ImageAdv, source_base: &Path, target_base: &Path) -> anyhow::Result<()> {
    let mut target = target_base.join(image.date.format("%Y-%m-%d").to_string());
    fs::create_dir_all(&target)
        .with_context(|| format!("Failed to create directory {}", target.display()))?;

    target.push(image.basic.get_name());

    if fs::exists(&target)? {
        bail!("File {} already exists", target.display());
    }

    let abs_path = source_base.join(&image.basic.path);
    fs::copy(&abs_path, &target).with_context(|| {
        format!(
            "Failed to copy to {} to {}",
            abs_path.display(),
            target.display()
        )
    })?;

    let new_len = fs::metadata(&target)?.len();

    if new_len != image.basic.size {
        fs::remove_file(&target)?;
        bail!("Length mismatch for {}", target.display());
    }

    Ok(())
}
