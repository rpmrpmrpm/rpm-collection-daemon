//! What the owner's computer uploads to the daemon.

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use crate::session::PeerKind;

/// One upload from the owner's computer: the catalogue for its own devices, the catalogue for guests,
/// and every key it trusts.
///
/// The catalogues are kept as the raw JSON that arrived, so they are sent on unchanged.
#[derive(Debug, Serialize, Deserialize)]
pub struct NasPublish {
    pub owner: Box<RawValue>,
    pub guest: Box<RawValue>,
    #[serde(default)]
    pub trust: Vec<NasTrusted>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NasTrusted {
    /// The reader's public key, base64url.
    pub public_key: String,
    pub kind: PeerKind,
}
