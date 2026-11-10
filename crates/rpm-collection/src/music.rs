//! Whether a folder holds music that rpm can read.

use std::fs;
use std::path::Path;

use crate::library::ID_PATH;

/// The file extensions rpm reads as music, lower case, without the dot. The same list as
/// `AudioFileScanner.AudioExtensions` in `RPM.Core`.
pub const AUDIO_EXTENSIONS: [&str; 15] = [
    "mp3", "m4a", "aac", "flac", "ogg", "oga", "opus", "wav", "aif", "aiff", "wma", "alac", "ape",
    "wv", "mpc",
];

/// How deep [`holds_music`] looks: `Artist/Album/Disc 1/track.flac` is found at this depth.
pub const MUSIC_DEPTH: usize = 4;

/// Whether a name is one that rpm and NAS systems keep for themselves: hidden names, and the folders
/// Synology and other systems make (`@eaDir`, `#recycle`, `#snapshot`).
pub fn is_system_name(name: &str) -> bool {
    name.starts_with('.') || name.starts_with('@') || name.starts_with('#')
}

/// Whether `folder` holds a music file, at most [`MUSIC_DEPTH`] folders down, outside any rpm library.
/// The search stops at the first one, so a large collection is answered after a few reads. Folders that
/// cannot be read are skipped.
pub fn holds_music(folder: &Path) -> bool {
    let mut pending = vec![(folder.to_path_buf(), 0usize)];
    while let Some((current, depth)) = pending.pop() {
        if current.join(ID_PATH).is_file() {
            continue;
        }
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if is_system_name(&name) {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_file() && is_audio(&name) {
                return true;
            }
            if kind.is_dir() && depth + 1 < MUSIC_DEPTH {
                pending.push((entry.path(), depth + 1));
            }
        }
    }
    false
}

fn is_audio(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, extension)| {
        AUDIO_EXTENSIONS
            .iter()
            .any(|known| known.eq_ignore_ascii_case(extension))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"x").unwrap();
    }

    #[test]
    fn finds_music_in_artist_and_album_folders() {
        let root = tempfile::tempdir().unwrap();
        touch(&root.path().join("Artist/Album/Disc 1/01 Song.FLAC"));
        assert!(holds_music(root.path()));
    }

    #[test]
    fn an_empty_folder_or_one_with_other_files_holds_none() {
        let root = tempfile::tempdir().unwrap();
        assert!(!holds_music(root.path()));
        touch(&root.path().join("Photos/holiday.jpg"));
        touch(&root.path().join("notes.txt"));
        assert!(!holds_music(root.path()));
    }

    #[test]
    fn music_in_a_library_hidden_or_system_folder_or_too_deep_does_not_count() {
        let root = tempfile::tempdir().unwrap();
        touch(&root.path().join("Chivan/.rpm/library.json"));
        touch(&root.path().join("Chivan/Artist/Album/song.mp3"));
        touch(&root.path().join("@eaDir/song.mp3/SYNOINDEX_MEDIA_INFO"));
        touch(&root.path().join("#recycle/old.mp3"));
        touch(&root.path().join(".hidden/song.mp3"));
        touch(&root.path().join("a/b/c/d/song.mp3"));
        assert!(!holds_music(root.path()));

        touch(&root.path().join("a/b/c/song.mp3"));
        assert!(holds_music(root.path()));
    }
}
