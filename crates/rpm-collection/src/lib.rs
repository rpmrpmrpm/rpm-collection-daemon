//! One rpm library folder on a NAS.
//!
//! The daemon does not index the folder. The owner's computer uploads the catalogue it already keeps,
//! with the keys it trusts, and this crate serves that catalogue as it arrived. A track is found by
//! looking up its path in the catalogue and serving that file from the folder.

pub mod claim;
pub mod covers;
pub mod index;
pub mod library;
pub mod media;
pub mod space;
pub mod store;

pub use library::Library;
pub use space::Space;
