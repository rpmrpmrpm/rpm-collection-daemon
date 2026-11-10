//! The protocol version and the routes.

/// The protocol version. It is `SharingProtocol.Version` in `RPM.Core`.
pub const WIRE_VERSION: i32 = 4;

/// Open to anyone: who this is.
pub const HELLO: &str = "/rpm/hello";
/// GET gives a challenge. POST with a signed challenge gives a token.
pub const SESSION: &str = "/rpm/session";
/// The catalogue, for a reader with a token.
pub const CATALOG: &str = "/rpm/catalog";
/// A track's audio, by rpm id. Followed by the id.
pub const TRACK: &str = "/rpm/track/";
/// An album's cover, asked for through any track of it. Followed by the rpm id.
pub const COVER: &str = "/rpm/cover/";
/// An album's disc picture, asked for through any track of it. Followed by the rpm id.
pub const DISC: &str = "/rpm/disc/";
/// A picture by key, such as `pl:12` for a playlist. Followed by the key.
pub const ART: &str = "/rpm/art/";

/// The owner's computer uploads the catalogues and its trust list here.
pub const NAS_PUBLISH: &str = "/rpm/nas/publish";
/// The owner's computer uploads a picture here. Followed by the key.
pub const NAS_ART: &str = "/rpm/nas/art/";

/// The routes whose URL may carry a media ticket instead of a token header.
pub const MEDIA: [&str; 4] = [TRACK, COVER, DISC, ART];
