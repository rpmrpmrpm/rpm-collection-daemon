//! The TLS certificate. It is self-signed over the device key, and readers check only that its public
//! key is the key they already hold for this device. There is no certificate authority and no expiry
//! to check.

use std::sync::Arc;

use rcgen::{
    CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::ServerConfig;
use time::{Duration, OffsetDateTime};

use crate::identity::Identity;

/// How long the certificate is valid for. Readers do not check it. It only has to be a valid
/// certificate.
const LIFE_DAYS: i64 = 3650;

/// The certificate for `identity`, as DER. Subject `CN=<deviceId>`, not a CA, key usage digital
/// signature and key agreement, extended key usage server authentication: the same as
/// `PeerCertificate.For` in `RPM.Core`.
pub fn certificate(identity: &Identity) -> Result<CertificateDer<'static>, rcgen::Error> {
    let key = KeyPair::try_from(identity.pkcs8())?;
    let mut params = CertificateParams::new(Vec::<String>::new())?;
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, identity.device_id());
    params.distinguished_name = name;
    params.is_ca = IsCa::ExplicitNoCa;
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyAgreement,
    ];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let now = OffsetDateTime::now_utc();
    params.not_before = now - Duration::days(1);
    params.not_after = now + Duration::days(LIFE_DAYS);
    Ok(params.self_signed(&key)?.der().clone())
}

/// The TLS server settings for `identity`: its certificate, HTTP/1.1 only.
pub fn server_config(
    identity: &Identity,
) -> Result<Arc<ServerConfig>, Box<dyn std::error::Error + Send + Sync>> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.pkcs8().to_vec()));
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(vec![certificate(identity)?], key)?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}
