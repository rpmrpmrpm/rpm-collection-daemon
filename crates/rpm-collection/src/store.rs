//! The uploads, kept on the disk so a restart serves what was served before.
//!
//! Each owner computer's latest upload is one file, written to a temporary name and then renamed. An
//! upload that is cut off never replaces the one before it.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

/// One owner computer's latest upload.
pub struct Stored {
    /// The device id of the computer that uploaded it.
    pub publisher: String,
    /// When it arrived, in milliseconds since 1970.
    pub at_ms: u64,
    /// The upload body as it arrived: a `NasPublish`.
    pub body: Vec<u8>,
}

#[derive(Serialize)]
struct FileOut<'a> {
    at: u64,
    publish: &'a RawValue,
}

#[derive(Deserialize)]
struct FileIn {
    at: u64,
    publish: Box<RawValue>,
}

const PREFIX: &str = "publisher-";
const SUFFIX: &str = ".json";

fn path_for(dir: &Path, publisher: &str) -> PathBuf {
    dir.join(format!("{PREFIX}{publisher}{SUFFIX}"))
}

/// Saves `body`, which must be JSON, as the latest upload from `publisher`.
pub fn save(dir: &Path, publisher: &str, at_ms: u64, body: &[u8]) -> io::Result<()> {
    let text =
        std::str::from_utf8(body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let raw: &RawValue = serde_json::from_str(text)?;
    let json = serde_json::to_vec(&FileOut {
        at: at_ms,
        publish: raw,
    })?;
    write_atomic(&path_for(dir, publisher), &json)
}

/// Every stored upload in `dir`. A file that cannot be read is skipped.
pub fn load_all(dir: &Path) -> Vec<Stored> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let publisher = name.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?.to_string();
            let file: FileIn = serde_json::from_slice(&fs::read(entry.path()).ok()?).ok()?;
            Some(Stored {
                publisher,
                at_ms: file.at,
                body: file.publish.get().as_bytes().to_vec(),
            })
        })
        .collect()
}

/// Writes `bytes` to `path` through a temporary file and a rename, so readers see the old file or the
/// new one and never half of one.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    let mut file = fs::File::create(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&temporary, path)
}
