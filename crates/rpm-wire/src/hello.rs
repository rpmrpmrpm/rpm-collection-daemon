//! The greeting at `/rpm/hello`, which anyone on the network may read.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerHello {
    pub version: i32,
    pub device_id: String,
    pub name: String,
    /// The public key, base64url.
    pub public_key: String,
    #[serde(default)]
    pub track_count: i32,
    /// The library this announcement serves. Absent for the NAS as a whole and for a computer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library_id: Option<String>,
    /// Space and libraries. Present only when a NAS answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nas: Option<NasHello>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NasHello {
    /// The SMB address of the folder that holds the libraries.
    pub smb: String,
    /// The space limit for all libraries together. Absent when there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_bytes: Option<u64>,
    pub used_bytes: u64,
    /// Free space on the disk that holds the libraries.
    pub free_bytes: u64,
    /// The libraries on the NAS. Empty in a library's own hello.
    #[serde(default)]
    pub libraries: Vec<NasLibraryEntry>,
    /// The folder holds music and no rpm library. A computer may make the folder itself a library, with
    /// the files as they are.
    #[serde(default)]
    pub existing_music: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NasLibraryEntry {
    /// The folder's name under the libraries folder. Empty when the library is that folder itself.
    pub folder: String,
    pub library_id: String,
    /// The name the owner's computer wrote into the claim file, or the folder name before a claim.
    pub name: String,
    /// The owner's account fingerprint from the claim file, or empty.
    pub account: String,
    /// The device id and public key this library is announced with.
    pub device_id: String,
    pub public_key: String,
}
