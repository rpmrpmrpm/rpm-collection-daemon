use rpm_peer::certificate::certificate;
use rpm_peer::identity::{id_for, verify, Identity};
use rpm_wire::base::{from_base64url, to_base64url};
use serde_json::Value;

fn vectors() -> Value {
    serde_json::from_str(include_str!("vectors/identity.json")).unwrap()
}

fn bytes(v: &Value, name: &str) -> Vec<u8> {
    from_base64url(v[name].as_str().unwrap()).unwrap()
}

#[test]
fn device_id_matches_dotnet_for_fixed_key() {
    let v = vectors();
    let identity = Identity::from_pkcs8(&bytes(&v, "pkcs8")).unwrap();
    assert_eq!(identity.device_id(), v["deviceId"].as_str().unwrap());
    assert_eq!(
        to_base64url(identity.public_key()),
        v["publicKey"].as_str().unwrap()
    );
    assert_eq!(id_for(identity.public_key()), identity.device_id());
}

#[test]
fn dotnet_signature_verifies() {
    let v = vectors();
    assert!(verify(
        &bytes(&v, "publicKey"),
        &bytes(&v, "data"),
        &bytes(&v, "signature")
    ));
    assert!(!verify(
        &bytes(&v, "publicKey"),
        b"other data",
        &bytes(&v, "signature")
    ));
}

#[test]
fn own_signature_is_64_bytes_and_verifies() {
    let identity = Identity::generate();
    let signature = identity.sign(b"hello");
    assert_eq!(signature.len(), 64);
    assert!(verify(identity.public_key(), b"hello", &signature));
}

#[test]
fn key_file_is_created_once_with_mode_0600() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("keys").join("peer.key");
    let first = Identity::load_or_create(&path).unwrap();
    let second = Identity::load_or_create(&path).unwrap();
    assert_eq!(first.device_id(), second.device_id());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn unreadable_key_file_is_an_error_not_a_new_key() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("peer.key");
    std::fs::write(&path, b"not a key").unwrap();
    assert!(Identity::load_or_create(&path).is_err());
}

#[test]
fn certificate_spki_is_the_device_key() {
    let identity = Identity::generate();
    let der = certificate(&identity).unwrap();
    let (_, parsed) = x509_parser::parse_x509_certificate(der.as_ref()).unwrap();
    assert_eq!(parsed.public_key().raw, identity.public_key());
    let cn = parsed
        .subject()
        .iter_common_name()
        .next()
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(cn, identity.device_id());
    assert!(!parsed.is_ca());
}
