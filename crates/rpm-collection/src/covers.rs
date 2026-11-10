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
/// A picture the person chose in rpm for an album of a library whose files are kept as they are. rpm
/// keeps those in `.rpm/art/<album folder>`, because the folder's own pictures may not be changed. The
/// same rule as `FolderArtWriter` in `RPM.Core`.
#[derive(Debug, PartialEq, Eq)]
pub enum Chosen {
    /// The chosen picture, read before the folder's own.
    Picture(PathBuf),
    /// The person removed the picture: the folder's own is not shown.
    Removed,
    /// Nothing was chosen.
    Nothing,
}

/// The chosen cover of `album`, a folder inside `library`.
pub fn chosen_cover(library: &Path, album: &Path) -> Chosen {
    chosen(library, album, &["cover.jpg", "cover.png"], "cover.removed")
}

/// The chosen disc picture of `album`, a folder inside `library`.
pub fn chosen_disc(library: &Path, album: &Path) -> Chosen {
    chosen(library, album, &DISC_NAMES, "disc.removed")
}

fn chosen(library: &Path, album: &Path, names: &[&str], removed: &str) -> Chosen {
    let Ok(relative) = album.strip_prefix(library) else {
        return Chosen::Nothing;
    };
    let folder = library.join(".rpm").join("art").join(relative);
    if folder.join(removed).is_file() {
        return Chosen::Removed;
    }
    names
        .iter()
        .map(|name| folder.join(name))
        .find(|path| path.is_file())
        .map_or(Chosen::Nothing, Chosen::Picture)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chosen_picture_comes_first_and_a_removal_hides_the_folders_own() {
        let library = tempfile::tempdir().unwrap();
        let album = library.path().join("Artist/Album");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("folder.jpg"), b"own").unwrap();
        let chosen = library.path().join(".rpm/art/Artist/Album");

        assert_eq!(chosen_cover(library.path(), &album), Chosen::Nothing);
        fs::create_dir_all(&chosen).unwrap();
        fs::write(chosen.join("cover.jpg"), b"chosen").unwrap();
        assert_eq!(
            chosen_cover(library.path(), &album),
            Chosen::Picture(chosen.join("cover.jpg"))
        );
        fs::write(chosen.join("cover.removed"), b"").unwrap();
        assert_eq!(chosen_cover(library.path(), &album), Chosen::Removed);

        fs::write(chosen.join("disc.png"), b"disc").unwrap();
        assert_eq!(
            chosen_disc(library.path(), &album),
            Chosen::Picture(chosen.join("disc.png"))
        );
        assert_eq!(
            chosen_cover(library.path(), &library.path().join("../elsewhere")),
            Chosen::Nothing
        );
    }
}
