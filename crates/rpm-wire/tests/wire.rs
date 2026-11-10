use rpm_wire::base::{crockford32, from_base64url, to_base64url};
use rpm_wire::{
    CatalogView, NasHello, NasPublish, PeerHello, PeerKind, SessionGrant, SessionProof,
};

#[test]
fn crockford_matches_dotnet_rule() {
    let bytes: Vec<u8> = (0u8..16).collect();
    assert_eq!(crockford32(&bytes), "000G40R40M30E209185GR38E1W");
}

#[test]
fn crockford_of_nothing_is_empty() {
    assert_eq!(crockford32(&[]), "");
}

#[test]
fn base64url_round_trips_without_padding() {
    let data = [0xfbu8, 0xff, 0xfe, 0x01];
    let text = to_base64url(&data);
    assert_eq!(text, "-__-AQ");
    assert_eq!(from_base64url(&text).unwrap(), data);
    assert_eq!(from_base64url("+//+AQ==").unwrap(), data);
    assert_eq!(from_base64url(""), None);
    assert_eq!(from_base64url("not base64!"), None);
}

#[test]
fn hello_serialises_camel_case_and_omits_nulls() {
    let hello = PeerHello {
        version: 4,
        device_id: "ID".into(),
        name: "DiskStation".into(),
        public_key: "KEY".into(),
        track_count: 0,
        library_id: None,
        nas: None,
    };
    let json = serde_json::to_string(&hello).unwrap();
    assert_eq!(
        json,
        r#"{"version":4,"deviceId":"ID","name":"DiskStation","publicKey":"KEY","trackCount":0}"#
    );
}

#[test]
fn nas_hello_omits_a_missing_limit() {
    let nas = NasHello {
        smb: "smb://nas/music/rpm".into(),
        limit_bytes: None,
        used_bytes: 10,
        free_bytes: 20,
        libraries: vec![],
    };
    let json = serde_json::to_string(&nas).unwrap();
    assert_eq!(
        json,
        r#"{"smb":"smb://nas/music/rpm","usedBytes":10,"freeBytes":20,"libraries":[]}"#
    );
}

#[test]
fn hello_from_dotnet_without_nas_fields_parses() {
    let hello: PeerHello = serde_json::from_str(
        r#"{"version":4,"deviceId":"ID","name":"Mac","publicKey":"KEY","trackCount":12}"#,
    )
    .unwrap();
    assert_eq!(hello.track_count, 12);
    assert!(hello.nas.is_none());
}

#[test]
fn grant_kind_is_a_number() {
    let grant: SessionGrant =
        serde_json::from_str(r#"{"token":"x","expiresIn":1800,"kind":1}"#).unwrap();
    assert_eq!(grant.kind, PeerKind::Guest);
    let back = serde_json::to_string(&grant).unwrap();
    assert!(back.contains(r#""kind":1"#));
}

#[test]
fn proof_without_account_reads_as_empty() {
    let proof: SessionProof =
        serde_json::from_str(r#"{"publicKey":"k","challenge":"c","signature":"s"}"#).unwrap();
    assert_eq!(proof.account, "");
}

#[test]
fn catalog_view_ignores_unknown_fields() {
    let json = include_bytes!("fixtures/catalog-full.json");
    let view = CatalogView::parse(json).unwrap();
    assert_eq!(view.version, "AbCdEfGhIjKlMnOp");
    assert_eq!(view.tracks.len(), 2);
    assert_eq!(view.tracks[0].rpm_id, "RPM0001");
    assert_eq!(view.tracks[0].path, "Artist/Album/01 First.flac");
    assert!(!view.tracks[0].cloud_only);
    assert!(view.tracks[1].cloud_only);
}

#[test]
fn publish_keeps_catalogues_byte_for_byte() {
    let catalogue = std::str::from_utf8(include_bytes!("fixtures/catalog-full.json"))
        .unwrap()
        .trim();
    let body = format!(
        r#"{{"owner":{catalogue},"guest":{catalogue},"trust":[{{"publicKey":"K","kind":0}}]}}"#
    );
    let publish: NasPublish = serde_json::from_str(&body).unwrap();
    assert_eq!(publish.owner.get(), catalogue);
    assert_eq!(publish.guest.get(), catalogue);
    assert_eq!(publish.trust[0].kind, PeerKind::Account);
}
