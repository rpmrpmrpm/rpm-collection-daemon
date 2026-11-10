//! Space: the limit, what the libraries use, and what the disk has free.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rpm_wire::{NasHello, NasLibraryEntry};

/// The space numbers for the folder that holds the libraries. One value is shared by the NAS and by
/// every library on it, because the limit is for all of them together.
pub struct Space {
    root: PathBuf,
    smb: String,
    limit: Option<u64>,
    used: AtomicU64,
    free: AtomicU64,
}

impl Space {
    pub fn new(root: PathBuf, smb: String, limit: Option<u64>) -> Space {
        Space {
            root,
            smb,
            limit,
            used: AtomicU64::new(0),
            free: AtomicU64::new(0),
        }
    }

    /// Measures again. It walks the whole folder, so call it off the request path.
    pub fn refresh(&self) {
        self.used.store(used(&self.root), Ordering::Relaxed);
        self.free.store(free(&self.root), Ordering::Relaxed);
    }

    pub fn used(&self) -> u64 {
        self.used.load(Ordering::Relaxed)
    }

    pub fn free(&self) -> u64 {
        self.free.load(Ordering::Relaxed)
    }

    pub fn limit(&self) -> Option<u64> {
        self.limit
    }

    /// The `nas` part of a hello.
    pub fn report(&self, libraries: Vec<NasLibraryEntry>) -> NasHello {
        NasHello {
            smb: self.smb.clone(),
            limit_bytes: self.limit,
            used_bytes: self.used(),
            free_bytes: self.free(),
            libraries,
        }
    }
}

/// The total size of the files under `root`. Symbolic links are not followed. Folders that cannot be
/// read are skipped.
pub fn used(root: &Path) -> u64 {
    let mut total = 0;
    let mut pending = vec![root.to_path_buf()];
    while let Some(folder) = pending.pop() {
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if meta.is_dir() {
                pending.push(entry.path());
            } else if meta.is_file() {
                total += meta.len();
            }
        }
    }
    total
}

/// The space free to an ordinary user on the disk that holds `path`.
#[cfg(unix)]
pub fn free(path: &Path) -> u64 {
    use std::os::unix::ffi::OsStrExt;
    let Ok(text) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return 0;
    };
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `text` is a valid C string and `stats` is a valid place for statvfs to write.
    if unsafe { libc::statvfs(text.as_ptr(), &mut stats) } != 0 {
        return 0;
    }
    #[allow(clippy::unnecessary_cast)]
    let free = stats.f_bavail as u64 * stats.f_frsize as u64;
    free
}

#[cfg(not(unix))]
pub fn free(_path: &Path) -> u64 {
    0
}
