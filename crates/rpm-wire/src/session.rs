//! Proving a key: a challenge, the signed answer, and the token it buys.

use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

/// How the owner's computer knows a reader. It is written as a number, because `RPM.Core` writes the
/// enum without a string converter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum PeerKind {
    /// Another device of the owner. It gets the owner's catalogue.
    Account = 0,
    /// Somebody else's device. It gets the guest catalogue.
    Guest = 1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionChallenge {
    pub challenge: String,
    pub expires_in: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionProof {
    /// The reader's public key, base64url.
    pub public_key: String,
    pub challenge: String,
    /// The signature over the signed bytes, base64url.
    pub signature: String,
    /// The account signed in on the reader, or empty. It is part of the signed bytes.
    #[serde(default)]
    pub account: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionGrant {
    pub token: String,
    pub expires_in: i32,
    pub kind: PeerKind,
}
