//! Album pictures from the library folder: the folder's image, the picture in a file, and the disc
//! picture. The names and their order are the ones `FolderArtWriter` in `RPM.Core` uses. They are
//! matched without regard to case, because the NAS disk is case-sensitive and the owner's computer is
//! usually not.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use lofty::config::ParseOptions;
use lofty::file::TaggedFileExt;
use lofty::picture::PictureType;
use lofty::probe::Probe;

const FOLDER_NAMES: [&str; 18] = [
    "cover.jpg",
    "cover.jpeg",
    "cover.png",
    "cover.webp",
    "folder.jpg",
    "folder.jpeg",
    "folder.png",
    "folder.webp",
    "front.jpg",
    "front.jpeg",
    "front.png",
    "album.jpg",
    "album.jpeg",
    "album.png",
    "albumart.jpg",
    "albumart.jpeg",
    "albumart.png",
    "albumartsmall.jpg",
];

const DISC_NAMES: [&str; 3] = ["disc.png", "disc.jpg", "disc.jpeg"];

const IMAGE_EXTENSIONS: [&str; 4] = ["jpg", "jpeg", "png", "webp"];

fn entries(folder: &Path) -> HashMap<String, PathBuf> {
    fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().to_lowercase(),
                entry.path(),
            )
        })
        .collect()
}

/// The folder's own cover: the first of the known names, or the only picture in the folder that is not
/// a disc picture. Two unnamed pictures give no answer, because there is no name to choose by.
pub fn folder_image(folder: &Path) -> Option<PathBuf> {
    let entries = entries(folder);
    if let Some(path) = FOLDER_NAMES.iter().find_map(|name| entries.get(*name)) {
        return Some(path.clone());
    }
    let mut pictures = entries.iter().filter(|(name, _)| {
        !DISC_NAMES.contains(&name.as_str())
            && Path::new(name.as_str())
                .extension()
                .map(|e| IMAGE_EXTENSIONS.contains(&e.to_string_lossy().as_ref()))
                .unwrap_or(false)
    });
    let only = pictures.next()?;
    pictures.next().is_none().then(|| only.1.clone())
}

/// The folder's disc picture.
pub fn disc_image(folder: &Path) -> Option<PathBuf> {
    let entries = entries(folder);
    DISC_NAMES
        .iter()
        .find_map(|name| entries.get(*name))
        .cloned()
}

/// The front cover in a file's tags, or else its first picture.
pub fn embedded(file: &Path) -> Option<Vec<u8>> {
    let tagged = Probe::open(file)
        .ok()?
        .options(ParseOptions::new().read_properties(false))
        .read()
        .ok()?;
    let pictures: Vec<_> = tagged
        .tags()
        .iter()
        .flat_map(|tag| tag.pictures())
        .collect();
    pictures
        .iter()
        .find(|picture| picture.pic_type() == PictureType::CoverFront)
        .or_else(|| pictures.first())
        .map(|picture| picture.data().to_vec())
        .filter(|data| !data.is_empty())
}

/// The content type of a picture, from its first bytes.
pub fn sniff(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else {
        "image/jpeg"
    }
}
