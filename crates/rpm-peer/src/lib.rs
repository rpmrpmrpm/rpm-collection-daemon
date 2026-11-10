//! An rpm peer: the device key and certificate, sessions, the trust check, the HTTPS server, and the
//! multicast DNS announcement. What is served is decided by the caller through [`http::Routes`].

pub mod beacon;
pub mod certificate;
pub mod http;
pub mod identity;
pub mod sessions;
pub mod trust;
