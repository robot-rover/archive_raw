use std::{ffi::OsStr, fs, path::Path};
use anyhow::{anyhow, bail};
use blake3::Hash;

use chrono::NaiveDateTime;
use rexiv2::Metadata;
use walkdir::{DirEntry, WalkDir};

const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "cr2"];

pub trait ImageExt: Sized {
    fn from_entry(entry: &DirEntry) -> anyhow::Result<Self>;
}

#[derive(Debug)]
pub struct ImageBasic {
    pub path: String,
    pub size: u64,
}

impl ImageExt for ImageBasic {
    fn from_entry(entry: &DirEntry) -> anyhow::Result<Self> {
        let path = entry.path()
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
            .map(|s| s.as_ref())
            .expect("Convertion from str to path and back failed")
    }
}

#[derive(Debug)]
pub struct ImageAdv {
    pub basic: ImageBasic,
    pub checksum: Hash,
    pub date: NaiveDateTime,
}

impl ImageAdv {
    pub fn from_basic(basic: ImageBasic) -> anyhow::Result<Self> {
        let content = fs::read(&basic.path)?;

        let metadata = Metadata::new_from_buffer(&content)?;
        if !metadata.has_exif() {
            bail!("No exif data found in {}", basic.path);
        }

        let Some(date) = metadata.get_tag_string("Exif.Image.DateTime")
            .ok()
            .and_then(|s| NaiveDateTime::parse_from_str(&s, "%Y:%m:%d %H:%M:%S").ok())
        else { bail!("Malformed or missing exif date found in {}", basic.path) } ;

        let checksum = blake3::hash(&content);

        Ok(ImageAdv { basic, checksum, date })
    }
}

impl ImageExt for ImageAdv {
    fn from_entry(entry: &DirEntry) -> anyhow::Result<Self> {
        ImageAdv::from_basic(ImageBasic::from_entry(entry)?)
    }
}

pub fn load_images<I: ImageExt>(dir: &Path) -> anyhow::Result<Vec<I>> {
    let mut vec = Vec::new();
    for entry in WalkDir::new(dir) {
        let entry = entry?;
        if !entry.file_type().is_file() { continue }
        for ext in IMAGE_EXTS {
            let file_name = entry.file_name();
            let local_path: &Path = file_name.as_ref();
            if local_path.extension() == Some(ext.as_ref()) {
                vec.push(I::from_entry(&entry)?);
                break;
            }
        }
    }

    Ok(vec)
}

pub fn archive_image(image: &ImageAdv, target_base: &Path) -> anyhow::Result<()> {
    let mut target = target_base.join(image.date.format("%Y-%m-%d").to_string());
    fs::create_dir_all(&target)?;
    target.push(image.basic.get_name());

    if fs::exists(&target)? {
        bail!("File {} already exists", target.display());
    }

    fs::copy(&image.basic.path, &target)?;

    let new_hash = blake3::hash(&fs::read(&target)?);

    if new_hash != image.checksum {
        fs::remove_file(&target)?;
        bail!("Checksum mismatch for {}", target.display());
    }

    Ok(())
}
