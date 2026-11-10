//! Where each track's file is, from the uploaded catalogue, and the rules for turning a catalogue path
//! into a file under the library folder.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use rpm_wire::CatalogView;
use unicode_normalization::UnicodeNormalization;

struct Entry {
    path: String,
    cloud_only: bool,
}

/// rpm id to path, and folder to the tracks in it.
#[derive(Default)]
pub struct Index {
    tracks: HashMap<String, Entry>,
    folders: HashMap<String, Vec<String>>,
}

impl Index {
    pub fn build(view: &CatalogView) -> Index {
        let mut index = Index::default();
        for track in &view.tracks {
            index
                .folders
                .entry(folder_of(&track.path).to_string())
                .or_default()
                .push(track.rpm_id.clone());
            index.tracks.insert(
                track.rpm_id.clone(),
                Entry {
                    path: track.path.clone(),
                    cloud_only: track.cloud_only,
                },
            );
        }
        index
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    /// The catalogue path of a track that has a file on the NAS. `None` for an unknown id and for a
    /// track the owner keeps only in the cloud.
    pub fn path_of(&self, rpm_id: &str) -> Option<&str> {
        let entry = self.tracks.get(rpm_id)?;
        (!entry.cloud_only).then_some(entry.path.as_str())
    }

    /// The paths of the other tracks in the same folder, for a cover that only one track carries.
    pub fn siblings(&self, rpm_id: &str) -> Vec<String> {
        let Some(path) = self.path_of(rpm_id) else {
            return Vec::new();
        };
        self.folders
            .get(folder_of(path))
            .into_iter()
            .flatten()
            .filter(|id| id.as_str() != rpm_id)
            .filter_map(|id| self.path_of(id).map(str::to_string))
            .collect()
    }

    /// The file for a track, inside `root`. See [`resolve_path`].
    pub fn resolve(&self, root: &Path, rpm_id: &str) -> Option<PathBuf> {
        resolve_path(root, self.path_of(rpm_id)?)
    }
}

fn folder_of(path: &str) -> &str {
    path.rsplit_once('/')
        .map(|(folder, _)| folder)
        .unwrap_or("")
}

/// The file at the catalogue path `relative` under `root`, or `None`.
///
/// `root` must already be canonical. A path that is absolute, has an empty, `.` or `..` part, or
/// holds a backslash is refused. The result is canonicalised and must still be inside `root`, so a
/// symbolic link that points out of the library is refused too.
///
/// The catalogue is written on the owner's computer, which is often case-insensitive and may use
/// either Unicode form for accented names. The NAS disk is usually case-sensitive. So when the exact
/// path is missing, each part is matched against the folder's entries by lower case NFC.
pub fn resolve_path(root: &Path, relative: &str) -> Option<PathBuf> {
    let parts: Vec<&str> = relative.split('/').collect();
    if parts
        .iter()
        .any(|part| part.is_empty() || *part == "." || *part == ".." || part.contains(['\\', '\0']))
    {
        return None;
    }
    let exact = parts
        .iter()
        .fold(root.to_path_buf(), |path, part| path.join(part));
    let found = if exact.exists() {
        exact
    } else {
        loose(root, &parts)?
    };
    let canonical = found.canonicalize().ok()?;
    (canonical.starts_with(root) && canonical.is_file()).then_some(canonical)
}

fn loose(root: &Path, parts: &[&str]) -> Option<PathBuf> {
    let mut current = root.to_path_buf();
    for part in parts {
        let exact = current.join(part);
        if exact.exists() {
            current = exact;
            continue;
        }
        let wanted = key(part);
        let entry = fs::read_dir(&current)
            .ok()?
            .flatten()
            .find(|entry| key(&entry.file_name().to_string_lossy()) == wanted)?;
        current = entry.path();
    }
    Some(current)
}

fn key(name: &str) -> String {
    name.nfc().collect::<String>().to_lowercase()
}
