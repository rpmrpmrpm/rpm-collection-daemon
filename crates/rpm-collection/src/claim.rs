//! The claim file: the keys of the computers that own a library. The owner's computer writes it over
//! SMB, so only someone who can write the library folder can claim it.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use rpm_wire::base::from_base64url;
use serde::{Deserialize, Serialize};

/// The claim file's path inside the library folder.
pub const CLAIM_PATH: &str = ".rpm/daemon-owner.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimFile {
    #[serde(default)]
    pub keys: Vec<ClaimKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimKey {
    /// The computer's public key, base64url.
    pub public_key: String,
    /// What the owner calls the library.
    #[serde(default)]
    pub name: String,
    /// The owner's account fingerprint, or empty when the computer was signed out.
    #[serde(default)]
    pub account: String,
}

impl ClaimFile {
    pub fn holds(&self, public_key: &[u8]) -> bool {
        self.keys
            .iter()
            .any(|key| from_base64url(&key.public_key).as_deref() == Some(public_key))
    }

    /// The name to show: the first non-empty name in the file.
    pub fn name(&self) -> Option<&str> {
        self.keys
            .iter()
            .map(|key| key.name.as_str())
            .find(|name| !name.is_empty())
    }

    /// The owner's account fingerprint: the first non-empty one in the file.
    pub fn account(&self) -> Option<&str> {
        self.keys
            .iter()
            .map(|key| key.account.as_str())
            .find(|account| !account.is_empty())
    }
}

/// Reads the claim file in `folder`. `None` when there is none or it cannot be read.
pub fn read(folder: &Path) -> Option<ClaimFile> {
    serde_json::from_slice(&fs::read(folder.join(CLAIM_PATH)).ok()?).ok()
}

/// The claim file, read again whenever its size or modified time changes. Asking costs one `stat`.
pub struct ClaimWatch {
    folder: PathBuf,
    cached: Mutex<Cached>,
}

#[derive(Default)]
struct Cached {
    stamp: Option<(SystemTime, u64)>,
    file: Option<ClaimFile>,
}

impl ClaimWatch {
    pub fn new(folder: PathBuf) -> ClaimWatch {
        ClaimWatch {
            folder,
            cached: Mutex::new(Cached::default()),
        }
    }

    pub fn current(&self) -> Option<ClaimFile> {
        let stamp = fs::metadata(self.folder.join(CLAIM_PATH))
            .ok()
            .and_then(|meta| Some((meta.modified().ok()?, meta.len())));
        let mut cached = self.cached.lock().unwrap();
        if cached.stamp != stamp || (stamp.is_some() && cached.file.is_none()) {
            cached.file = stamp.and_then(|_| read(&self.folder));
            cached.stamp = stamp;
        }
        cached.file.clone()
    }
}
