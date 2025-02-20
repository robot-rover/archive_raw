use std::{ffi::OsStr, path::Path};
use anyhow::anyhow;
use blake3::Hash;

use chrono::{NaiveDateTime, TimeDelta};
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
    pub fn get_name(&self) -> anyhow::Result<&str> {
        AsRef::<Path>::as_ref(&self.path)
            .file_name()
            .and_then(OsStr::to_str)
            .map(|s| s.as_ref())
            .ok_or_else(|| anyhow!("Invalid path encountered: {}", self.path))
    }
}

#[derive(Debug)]
pub struct ImageAdv {
    pub basic: ImageBasic,
    pub checksum: Hash,
    pub taken: Option<NaiveDateTime>,
    pub duration: Option<TimeDelta>,
}

impl ImageAdv {
    pub fn from_basic(basic: ImageBasic) -> anyhow::Result<Self> {
        //let file = fs::File::open(basic.path)?;

        let mut hasher = blake3::Hasher::new();
        hasher.update_mmap(&basic.path)?;
        let checksum = hasher.finalize();

        // TODO: read EXIF data and video duration
        Ok(ImageAdv {
            basic,
            checksum,
            taken: None,
            duration: None,
        })
    }
}

impl ImageExt for ImageAdv {
    fn from_entry(entry: &DirEntry) -> anyhow::Result<Self> {
        Self::from_basic(ImageBasic::from_entry(entry)?)
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

