//! The multicast DNS announcement: the service type and the TXT record keys.

/// The service type for a NAS and for each library on it. It is not the type that rpm computers use
/// (`_rpm._tcp`), so an rpm build that does not know about a NAS never sees one.
pub const SERVICE_TYPE: &str = "_rpm-nas._tcp";

/// The protocol version.
pub const VERSION: &str = "v";
/// The device id.
pub const DEVICE_ID: &str = "id";
/// The name to show.
pub const NAME: &str = "n";
/// The public key, base64url.
pub const PUBLIC_KEY: &str = "k";
/// What the announcement is: [`ROLE_NAS`] or [`ROLE_LIBRARY`].
pub const ROLE: &str = "r";
/// The library id. Only in a library's announcement.
pub const LIBRARY_ID: &str = "l";
/// The SMB address of the folder that holds the libraries. Only in the NAS's announcement.
pub const SMB: &str = "s";

/// The NAS as a whole. It serves no catalogue. Its hello lists the libraries.
pub const ROLE_NAS: &str = "nas";
/// One library folder on the NAS.
pub const ROLE_LIBRARY: &str = "library";
