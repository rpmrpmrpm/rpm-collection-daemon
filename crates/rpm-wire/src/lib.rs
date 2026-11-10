//! The rpm peer protocol, as far as the collection daemon reads and writes it.
//!
//! The protocol is defined by `RPM.Core/Sharing` in the `rpm-desktop` repo. These types follow its JSON
//! exactly: camelCase names, and a value that is absent is left out rather than written as null.
//!
//! The catalogue is not modelled here in full. The daemon stores the catalogue that the owner's computer
//! uploads and sends it on byte for byte, so fields that rpm adds later reach the readers without a
//! change here. [`catalog::CatalogView`] reads only the fields the daemon needs to find a track's file.

pub mod base;
pub mod catalog;
pub mod hello;
pub mod nas;
pub mod routes;
pub mod session;
pub mod txt;

pub use catalog::{CatalogView, TrackRef};
pub use hello::{NasHello, NasLibraryEntry, PeerHello};
pub use nas::{NasPublish, NasTrusted};
pub use session::{PeerKind, SessionChallenge, SessionGrant, SessionProof};
