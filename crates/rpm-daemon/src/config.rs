//! The configuration: one TOML file, and environment variables that override it.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The folder that holds the libraries. Each subfolder is one person's library.
    pub root: PathBuf,
    /// The space limit for all libraries together, in bytes. `None` for no limit.
    pub limit: Option<u64>,
    /// The NAS name to announce.
    pub name: String,
    /// Where the daemon keeps its keys and the uploads. Never inside `root`.
    pub data: PathBuf,
    /// How computers reach `root` over SMB, for example `smb://nas.local/music/rpm`.
    pub smb_share: String,
    /// The first port to listen on: the NAS takes this one and the libraries the ones after it. `None`
    /// lets the system choose, which is fine unless a firewall on the NAS needs fixed ports.
    pub port: Option<u16>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ConfigError(pub String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ConfigError {}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    root: Option<PathBuf>,
    limit: Option<Limit>,
    name: Option<String>,
    data: Option<PathBuf>,
    smb_share: Option<String>,
    port: Option<u16>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Limit {
    Bytes(u64),
    Text(String),
}

/// Reads the configuration from the file at `path`, if given, and then from the environment
/// (`RPM_ROOT`, `RPM_LIMIT`, `RPM_NAME`, `RPM_DATA`, `RPM_SMB_SHARE`, `RPM_PORT`), which wins.
///
/// `env` is how the environment is read, so a test can pass its own.
pub fn load(
    path: Option<&Path>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Config, ConfigError> {
    let file: FileConfig = match path {
        Some(path) => {
            let text = fs::read_to_string(path)
                .map_err(|e| ConfigError(format!("{} could not be read: {e}", path.display())))?;
            toml::from_str(&text)
                .map_err(|e| ConfigError(format!("{} is not valid: {e}", path.display())))?
        }
        None => FileConfig::default(),
    };
    let env = |name: &str| env(name).filter(|value| !value.trim().is_empty());

    let root = env("RPM_ROOT")
        .map(PathBuf::from)
        .or(file.root)
        .ok_or_else(|| ConfigError("root is not set (RPM_ROOT)".into()))?;
    let data = env("RPM_DATA")
        .map(PathBuf::from)
        .or(file.data)
        .ok_or_else(|| ConfigError("data is not set (RPM_DATA)".into()))?;
    let smb_share = env("RPM_SMB_SHARE")
        .or(file.smb_share)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ConfigError("smb_share is not set (RPM_SMB_SHARE)".into()))?;
    let limit = match env("RPM_LIMIT") {
        Some(text) => {
            parse_size(&text).ok_or_else(|| ConfigError(format!("limit {text:?} is not a size")))?
        }
        None => match file.limit {
            Some(Limit::Bytes(bytes)) => Some(bytes),
            Some(Limit::Text(text)) => parse_size(&text)
                .ok_or_else(|| ConfigError(format!("limit {text:?} is not a size")))?,
            None => None,
        },
    };
    let name = env("RPM_NAME")
        .or(file.name)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(host_name);
    let port = match env("RPM_PORT") {
        Some(text) => Some(
            text.trim()
                .parse()
                .map_err(|_| ConfigError(format!("port {text:?} is not a port number")))?,
        ),
        None => file.port,
    };

    if !root.is_dir() {
        return Err(ConfigError(format!(
            "root {} is not a folder",
            root.display()
        )));
    }
    fs::create_dir_all(&data)
        .map_err(|e| ConfigError(format!("data {} could not be made: {e}", data.display())))?;
    let root = root
        .canonicalize()
        .map_err(|e| ConfigError(format!("root {}: {e}", root.display())))?;
    let data = data
        .canonicalize()
        .map_err(|e| ConfigError(format!("data {}: {e}", data.display())))?;
    // Keys and uploads inside root would be visible over SMB, and would be scanned as a library.
    if data.starts_with(&root) {
        return Err(ConfigError("data must not be inside root".into()));
    }

    Ok(Config {
        root,
        limit,
        name,
        data,
        smb_share,
        port,
    })
}

/// A size such as `2TB`, `500 GB`, `1.5TB`, `100GiB` or a number of bytes. KB, MB, GB and TB are
/// powers of 1000, as disk sizes are sold; KiB, MiB, GiB and TiB are powers of 1024. An empty text is
/// "no limit" and gives `Some(None)`.
pub fn parse_size(text: &str) -> Option<Option<u64>> {
    let text = text.trim();
    if text.is_empty() {
        return Some(None);
    }
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: f64 = number.parse().ok()?;
    let factor: f64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1.0,
        "kb" | "k" => 1e3,
        "mb" | "m" => 1e6,
        "gb" | "g" => 1e9,
        "tb" | "t" => 1e12,
        "kib" => 1024f64,
        "mib" => 1024f64.powi(2),
        "gib" => 1024f64.powi(3),
        "tib" => 1024f64.powi(4),
        _ => return None,
    };
    let bytes = number * factor;
    (bytes.is_finite() && bytes >= 0.0 && bytes < u64::MAX as f64).then_some(Some(bytes as u64))
}

/// This machine's host name, without a domain.
pub fn host_name() -> String {
    let name = gethostname::gethostname().to_string_lossy().into_owned();
    let short = name.split('.').next().unwrap_or("").trim().to_string();
    if short.is_empty() {
        "NAS".to_string()
    } else {
        short
    }
}
