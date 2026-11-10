//! The part of a catalogue that the daemon reads.

use serde::Deserialize;

/// The fields of an uploaded catalogue that the daemon needs: its version, and where each track's file
/// is. Every other field is ignored here and kept in the stored bytes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CatalogView {
    pub version: String,
    #[serde(default)]
    pub tracks: Vec<TrackRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackRef {
    pub rpm_id: String,
    /// The file's path under the library folder, with `/` between folders.
    pub path: String,
    /// The owner keeps this track only in the cloud. There is no file on the NAS.
    #[serde(default)]
    pub cloud_only: bool,
}

impl CatalogView {
    pub fn parse(json: &[u8]) -> Result<CatalogView, serde_json::Error> {
        serde_json::from_slice(json)
    }
}
