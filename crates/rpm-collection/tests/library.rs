use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use http_body_util::BodyExt;
use hyper::header::{HeaderMap, HeaderValue, CONTENT_RANGE, CONTENT_TYPE, RANGE};
use hyper::{Method, StatusCode};
use rpm_collection::claim::{ClaimFile, ClaimKey, CLAIM_PATH};
use rpm_collection::index::Index;
use rpm_collection::media::parse_range;
use rpm_collection::{Library, Space};
use rpm_peer::http::{AuthedRequest, Routes};
use rpm_peer::identity::Identity;
use rpm_peer::trust::{standing, Reader, Trust};
use rpm_wire::base::to_base64url;
use rpm_wire::{CatalogView, PeerKind};
use tempfile::TempDir;

const LIBRARY_ID: &str = "0123456789abcdef0123456789abcdef";
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfolder picture";

struct Fixture {
    _temp: TempDir,
    temp_path: PathBuf,
    folder: PathBuf,
    data: PathBuf,
    space: Arc<Space>,
    owner: Identity,
    library: Arc<Library>,
}

fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let temp_path = temp.path().canonicalize().unwrap();
    let root = temp_path.join("root");
    let folder = root.join("Chivan");
    fs::create_dir_all(folder.join(".rpm")).unwrap();
    fs::write(
        folder.join(".rpm/library.json"),
        format!("{{\"id\":\"{LIBRARY_ID}\"}}\n"),
    )
    .unwrap();
    let owner = Identity::generate();
    claim(&folder, &[&owner], "Chivan");
    let data = temp_path.join("data");
    let space = Arc::new(Space::new(root, "smb://nas/music/rpm".into(), Some(1000)));
    let library = Library::open(&folder, &data, space.clone()).unwrap();
    Fixture {
        _temp: temp,
        temp_path,
        folder,
        data,
        space,
        owner,
        library,
    }
}

fn claim(folder: &Path, owners: &[&Identity], name: &str) {
    let file = ClaimFile {
        keys: owners
            .iter()
            .map(|o| ClaimKey {
                public_key: to_base64url(o.public_key()),
                name: name.into(),
                account: "acct".into(),
            })
            .collect(),
    };
    fs::write(folder.join(CLAIM_PATH), serde_json::to_vec(&file).unwrap()).unwrap();
}

fn write(folder: &Path, relative: &str, bytes: &[u8]) -> PathBuf {
    let path = folder.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    path
}

fn catalogue(version: &str, tracks: &[(&str, &str, bool)]) -> String {
    let rows: Vec<String> = tracks
        .iter()
        .map(|(id, path, cloud)| {
            serde_json::json!({"rpmId": id, "path": path, "cloudOnly": cloud, "title": "t"})
                .to_string()
        })
        .collect();
    format!(
        r#"{{"version":"{version}","name":"x","tracks":[{}],"albums":[]}}"#,
        rows.join(",")
    )
}

fn upload(owner: &str, guest: &str, trust: &[(&[u8], PeerKind)]) -> Vec<u8> {
    let trust: Vec<String> = trust
        .iter()
        .map(|(key, kind)| {
            format!(
                r#"{{"publicKey":"{}","kind":{}}}"#,
                to_base64url(key),
                *kind as u8
            )
        })
        .collect();
    format!(
        r#"{{"owner":{owner},"guest":{guest},"trust":[{}]}}"#,
        trust.join(",")
    )
    .into_bytes()
}

fn as_reader(library: &Library, key: &Identity) -> Reader {
    standing(library, key.public_key()).expect("a trusted key")
}

fn request(method: Method, path: &str, reader: Reader) -> AuthedRequest {
    AuthedRequest {
        method,
        path: path.to_string(),
        query: Default::default(),
        headers: HeaderMap::new(),
        body: Bytes::new(),
        reader,
    }
}

async fn call(library: &Library, request: AuthedRequest) -> (StatusCode, HeaderMap, Bytes) {
    let response = library.handle(request).await;
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, headers, body)
}

async fn publish(f: &Fixture, by: &Identity, body: Vec<u8>) -> StatusCode {
    let mut r = request(Method::POST, "/rpm/nas/publish", as_reader(&f.library, by));
    r.body = body.into();
    call(&f.library, r).await.0
}

async fn get(f: &Fixture, path: &str, reader: Reader) -> (StatusCode, HeaderMap, Bytes) {
    call(&f.library, request(Method::GET, path, reader)).await
}

/// Publishes one catalogue with these tracks, trusting `guest` as a guest.
async fn publish_tracks(f: &Fixture, tracks: &[(&str, &str, bool)], guest: &Identity) {
    let owner = catalogue("V1", tracks);
    let guests = catalogue("G1", tracks);
    let status = publish(
        f,
        &f.owner,
        upload(&owner, &guests, &[(guest.public_key(), PeerKind::Guest)]),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

fn guest_reader(f: &Fixture, guest: &Identity) -> Reader {
    as_reader(&f.library, guest)
}

#[tokio::test]
async fn publish_from_unclaimed_key_is_403() {
    let f = fixture();
    let stranger = Identity::generate();
    assert!(standing(&*f.library, stranger.public_key()).is_none());
    let reader = Reader {
        public_key: stranger.public_key().to_vec(),
        kind: PeerKind::Account,
        owner: false,
    };
    let mut r = request(Method::POST, "/rpm/nas/publish", reader);
    r.body = upload(&catalogue("V", &[]), &catalogue("G", &[]), &[]).into();
    assert_eq!(call(&f.library, r).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn catalogue_before_any_upload_is_503() {
    let f = fixture();
    let owner = as_reader(&f.library, &f.owner);
    assert_eq!(
        get(&f, "/rpm/catalog", owner).await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn publish_from_owner_key_is_204_and_served() {
    let f = fixture();
    let owner_json = catalogue("V1", &[("A", "a.flac", false)]);
    let status = publish(
        &f,
        &f.owner,
        upload(&owner_json, &catalogue("G1", &[]), &[]),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, headers, body) = get(&f, "/rpm/catalog", as_reader(&f.library, &f.owner)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, owner_json.as_bytes());
    assert!(headers[CONTENT_TYPE]
        .to_str()
        .unwrap()
        .starts_with("application/json"));
}

#[tokio::test]
async fn guest_gets_guest_variant_owner_kind_gets_owner_variant() {
    let f = fixture();
    let guest = Identity::generate();
    let own_device = Identity::generate();
    let owner_json = catalogue("OWN", &[]);
    let guest_json = catalogue("GUEST", &[]);
    publish(
        &f,
        &f.owner,
        upload(
            &owner_json,
            &guest_json,
            &[
                (guest.public_key(), PeerKind::Guest),
                (own_device.public_key(), PeerKind::Account),
            ],
        ),
    )
    .await;
    let (_, _, body) = get(&f, "/rpm/catalog", as_reader(&f.library, &guest)).await;
    assert_eq!(body, guest_json.as_bytes());
    let (_, _, body) = get(&f, "/rpm/catalog", as_reader(&f.library, &own_device)).await;
    assert_eq!(body, owner_json.as_bytes());
}

#[tokio::test]
async fn catalog_with_current_have_is_204() {
    let f = fixture();
    let guest = Identity::generate();
    publish_tracks(&f, &[], &guest).await;
    let mut r = request(Method::GET, "/rpm/catalog", guest_reader(&f, &guest));
    r.query.insert("have".into(), "G1".into());
    assert_eq!(call(&f.library, r).await.0, StatusCode::NO_CONTENT);
    let mut r = request(Method::GET, "/rpm/catalog", guest_reader(&f, &guest));
    r.query.insert("have".into(), "OLD".into());
    assert_eq!(call(&f.library, r).await.0, StatusCode::OK);
}

#[tokio::test]
async fn two_publishing_keys_union_their_trust_lists() {
    let f = fixture();
    let second = Identity::generate();
    claim(&f.folder, &[&f.owner, &second], "Chivan");
    let tv = Identity::generate();
    let friend = Identity::generate();
    publish(
        &f,
        &f.owner,
        upload(
            &catalogue("A", &[]),
            &catalogue("A", &[]),
            &[(tv.public_key(), PeerKind::Account)],
        ),
    )
    .await;
    publish(
        &f,
        &second,
        upload(
            &catalogue("B", &[]),
            &catalogue("B", &[]),
            &[(friend.public_key(), PeerKind::Guest)],
        ),
    )
    .await;
    assert_eq!(f.library.kind_of(tv.public_key()), Some(PeerKind::Account));
    assert_eq!(
        f.library.kind_of(friend.public_key()),
        Some(PeerKind::Guest)
    );
}

#[tokio::test]
async fn catalog_is_latest_publish_from_either_key() {
    let f = fixture();
    let second = Identity::generate();
    claim(&f.folder, &[&f.owner, &second], "Chivan");
    publish(
        &f,
        &f.owner,
        upload(&catalogue("A", &[]), &catalogue("A", &[]), &[]),
    )
    .await;
    std::thread::sleep(std::time::Duration::from_millis(5));
    publish(
        &f,
        &second,
        upload(&catalogue("B", &[]), &catalogue("B", &[]), &[]),
    )
    .await;
    let (_, _, body) = get(&f, "/rpm/catalog", as_reader(&f.library, &f.owner)).await;
    assert!(String::from_utf8_lossy(&body).contains(r#""version":"B""#));
}

#[tokio::test]
async fn removed_key_stops_reading_after_the_next_upload() {
    let f = fixture();
    let guest = Identity::generate();
    publish_tracks(&f, &[], &guest).await;
    assert!(f.library.kind_of(guest.public_key()).is_some());
    publish(
        &f,
        &f.owner,
        upload(&catalogue("V2", &[]), &catalogue("G2", &[]), &[]),
    )
    .await;
    assert!(f.library.kind_of(guest.public_key()).is_none());
}

#[tokio::test]
async fn track_streams_whole_with_its_type() {
    let f = fixture();
    let guest = Identity::generate();
    write(&f.folder, "Artist/Album/01.flac", b"0123456789");
    publish_tracks(&f, &[("A", "Artist/Album/01.flac", false)], &guest).await;
    let (status, headers, body) = get(&f, "/rpm/track/A", guest_reader(&f, &guest)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, &b"0123456789"[..]);
    assert_eq!(headers[CONTENT_TYPE], "audio/flac");
    assert_eq!(headers["accept-ranges"], "bytes");
}

#[tokio::test]
async fn cloud_only_track_is_404() {
    let f = fixture();
    let guest = Identity::generate();
    write(&f.folder, "a.flac", b"x");
    publish_tracks(&f, &[("A", "a.flac", true)], &guest).await;
    assert_eq!(
        get(&f, "/rpm/track/A", guest_reader(&f, &guest)).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn missing_file_is_404() {
    let f = fixture();
    let guest = Identity::generate();
    publish_tracks(&f, &[("A", "gone.flac", false)], &guest).await;
    assert_eq!(
        get(&f, "/rpm/track/A", guest_reader(&f, &guest)).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&f, "/rpm/track/UNKNOWN", guest_reader(&f, &guest))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[test]
fn range_matches_byterange_rules() {
    assert_eq!(parse_range(Some("bytes=0-99"), 1000), Some((0, 99)));
    assert_eq!(parse_range(Some("bytes=-500"), 1000), Some((500, 999)));
    assert_eq!(parse_range(Some("bytes=-5000"), 1000), Some((0, 999)));
    assert_eq!(parse_range(Some("bytes=10-"), 1000), Some((10, 999)));
    assert_eq!(parse_range(Some("bytes=10-5000"), 1000), Some((10, 999)));
    assert_eq!(parse_range(Some("BYTES=1-2"), 1000), Some((1, 2)));
    assert_eq!(parse_range(Some("bytes=5-2"), 1000), None);
    assert_eq!(parse_range(Some("bytes=0-1,4-5"), 1000), None);
    assert_eq!(parse_range(Some("bytes=1000-"), 1000), None);
    assert_eq!(parse_range(Some("items=0-1"), 1000), None);
    assert_eq!(parse_range(Some("bytes=-0"), 1000), None);
    assert_eq!(parse_range(None, 1000), None);
    assert_eq!(parse_range(Some("bytes=0-1"), 0), None);
}

#[tokio::test]
async fn range_request_is_206_with_content_range() {
    let f = fixture();
    let guest = Identity::generate();
    write(&f.folder, "a.mp3", b"0123456789");
    publish_tracks(&f, &[("A", "a.mp3", false)], &guest).await;
    let mut r = request(Method::GET, "/rpm/track/A", guest_reader(&f, &guest));
    r.headers
        .insert(RANGE, HeaderValue::from_static("bytes=2-5"));
    let (status, headers, body) = call(&f.library, r).await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(body, &b"2345"[..]);
    assert_eq!(headers[CONTENT_RANGE], "bytes 2-5/10");
    assert_eq!(headers[CONTENT_TYPE], "audio/mpeg");

    let mut r = request(Method::GET, "/rpm/track/A", guest_reader(&f, &guest));
    r.headers
        .insert(RANGE, HeaderValue::from_static("bytes=0-1,4-5"));
    let (status, _, body) = call(&f.library, r).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.len(), 10);
}

#[tokio::test]
async fn head_sends_headers_only() {
    let f = fixture();
    let guest = Identity::generate();
    write(&f.folder, "a.flac", b"0123456789");
    publish_tracks(&f, &[("A", "a.flac", false)], &guest).await;
    let (status, headers, body) = call(
        &f.library,
        request(Method::HEAD, "/rpm/track/A", guest_reader(&f, &guest)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["content-length"], "10");
    assert!(body.is_empty());
}

#[test]
fn content_type_follows_extension() {
    use rpm_collection::media::content_type;
    for (name, kind) in [
        ("a.MP3", "audio/mpeg"),
        ("a.m4b", "audio/mp4"),
        ("a.oga", "audio/ogg"),
        ("a.aif", "audio/aiff"),
        ("a.wv", "audio/x-wavpack"),
        ("a.xyz", "application/octet-stream"),
        ("noextension", "application/octet-stream"),
    ] {
        assert_eq!(content_type(Path::new(name)), kind, "{name}");
    }
}

#[tokio::test]
async fn cover_prefers_folder_image_in_name_order() {
    let f = fixture();
    let guest = Identity::generate();
    write(&f.folder, "Album/01.flac", b"x");
    write(&f.folder, "Album/Folder.JPG", b"\xff\xd8folder");
    write(&f.folder, "Album/cover.png", PNG);
    publish_tracks(&f, &[("A", "Album/01.flac", false)], &guest).await;
    let (status, headers, body) = get(&f, "/rpm/cover/A", guest_reader(&f, &guest)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, PNG);
    assert_eq!(headers[CONTENT_TYPE], "image/png");
    assert!(headers["cache-control"]
        .to_str()
        .unwrap()
        .contains("immutable"));
}

#[tokio::test]
async fn cover_name_is_matched_without_case() {
    let f = fixture();
    let guest = Identity::generate();
    write(&f.folder, "Album/01.flac", b"x");
    write(&f.folder, "Album/Cover.JPG", b"\xff\xd8folder");
    publish_tracks(&f, &[("A", "Album/01.flac", false)], &guest).await;
    let (_, headers, body) = get(&f, "/rpm/cover/A", guest_reader(&f, &guest)).await;
    assert_eq!(body, &b"\xff\xd8folder"[..]);
    assert_eq!(headers[CONTENT_TYPE], "image/jpeg");
}

#[tokio::test]
async fn cover_falls_back_to_single_unnamed_image() {
    let f = fixture();
    let guest = Identity::generate();
    write(&f.folder, "Album/01.flac", b"x");
    write(&f.folder, "Album/scan.png", PNG);
    write(&f.folder, "Album/disc.png", b"\x89PNGdisc");
    publish_tracks(&f, &[("A", "Album/01.flac", false)], &guest).await;
    assert_eq!(
        get(&f, "/rpm/cover/A", guest_reader(&f, &guest)).await.2,
        PNG
    );

    write(&f.folder, "Album/back.png", PNG);
    assert_eq!(
        get(&f, "/rpm/cover/A", guest_reader(&f, &guest)).await.0,
        StatusCode::NOT_FOUND
    );
}

/// An MP3 of silent frames with a front cover in its ID3v2 tag.
fn mp3_with_picture(path: &Path, picture: &[u8]) {
    use lofty::config::WriteOptions;
    use lofty::id3::v2::Id3v2Tag;
    use lofty::picture::{MimeType, Picture, PictureType};
    use lofty::tag::TagExt;

    let mut frames = Vec::new();
    for _ in 0..10 {
        let mut frame = vec![0u8; 417];
        frame[..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x64]);
        frames.extend_from_slice(&frame);
    }
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, frames).unwrap();
    let mut tag = Id3v2Tag::default();
    tag.insert_picture(Picture::new_unchecked(
        PictureType::CoverFront,
        Some(MimeType::Png),
        None,
        picture.to_vec(),
    ));
    tag.save_to_path(path, WriteOptions::default()).unwrap();
}

#[tokio::test]
async fn cover_falls_back_to_embedded_art_then_sibling() {
    let f = fixture();
    let guest = Identity::generate();
    write(&f.folder, "Album/01.mp3", b"no tags here");
    mp3_with_picture(&f.folder.join("Album/09.mp3"), PNG);
    publish_tracks(
        &f,
        &[("A", "Album/01.mp3", false), ("B", "Album/09.mp3", false)],
        &guest,
    )
    .await;
    let (status, _, body) = get(&f, "/rpm/cover/B", guest_reader(&f, &guest)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, PNG);
    let (status, _, body) = get(&f, "/rpm/cover/A", guest_reader(&f, &guest)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, PNG);
}

#[tokio::test]
async fn disc_uses_disc_names_only() {
    let f = fixture();
    let guest = Identity::generate();
    write(&f.folder, "Album/01.flac", b"x");
    write(&f.folder, "Album/cover.png", PNG);
    publish_tracks(&f, &[("A", "Album/01.flac", false)], &guest).await;
    assert_eq!(
        get(&f, "/rpm/disc/A", guest_reader(&f, &guest)).await.0,
        StatusCode::NOT_FOUND
    );
    write(&f.folder, "Album/Disc.png", b"\x89PNGdisc");
    assert_eq!(
        get(&f, "/rpm/disc/A", guest_reader(&f, &guest)).await.2,
        &b"\x89PNGdisc"[..]
    );
}

#[tokio::test]
async fn art_returns_uploaded_picture() {
    let f = fixture();
    let guest = Identity::generate();
    publish_tracks(&f, &[], &guest).await;
    let mut put = request(
        Method::PUT,
        "/rpm/nas/art/pl:12",
        as_reader(&f.library, &f.owner),
    );
    put.body = Bytes::from_static(PNG);
    assert_eq!(call(&f.library, put).await.0, StatusCode::NO_CONTENT);
    let (status, _, body) = get(&f, "/rpm/art/pl:12", guest_reader(&f, &guest)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, PNG);
    assert_eq!(
        get(&f, "/rpm/art/pl:13", guest_reader(&f, &guest)).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn art_upload_needs_an_owner_and_a_plain_key() {
    let f = fixture();
    let guest = Identity::generate();
    publish_tracks(&f, &[], &guest).await;
    let mut put = request(Method::PUT, "/rpm/nas/art/pl:12", guest_reader(&f, &guest));
    put.body = Bytes::from_static(PNG);
    assert_eq!(call(&f.library, put).await.0, StatusCode::FORBIDDEN);
    let mut put = request(
        Method::PUT,
        "/rpm/nas/art/..%2Fx",
        as_reader(&f.library, &f.owner),
    );
    put.body = Bytes::from_static(PNG);
    assert_eq!(call(&f.library, put).await.0, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn art_over_4_mb_is_413() {
    let f = fixture();
    let mut put = request(
        Method::PUT,
        "/rpm/nas/art/pl:1",
        as_reader(&f.library, &f.owner),
    );
    put.body = Bytes::from(vec![0u8; 4 * 1024 * 1024 + 1]);
    assert_eq!(call(&f.library, put).await.0, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn track_path_outside_library_is_refused() {
    let f = fixture();
    let guest = Identity::generate();
    write(&f.temp_path, "root/secret.flac", b"secret");
    write(&f.temp_path, "outside.flac", b"secret");
    let outside = f.temp_path.join("outside.flac");
    publish_tracks(
        &f,
        &[
            ("UP", "../secret.flac", false),
            ("ABS", outside.to_str().unwrap(), false),
            ("DOT", "./a.flac", false),
            ("BACK", "Album\\..\\..\\secret.flac", false),
        ],
        &guest,
    )
    .await;
    for id in ["UP", "ABS", "DOT", "BACK"] {
        let path = format!("/rpm/track/{id}");
        assert_eq!(
            get(&f, &path, guest_reader(&f, &guest)).await.0,
            StatusCode::NOT_FOUND,
            "{id}"
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_out_of_library_is_refused() {
    let f = fixture();
    let guest = Identity::generate();
    let outside = write(&f.temp_path, "outside.flac", b"secret");
    fs::create_dir_all(f.folder.join("Album")).unwrap();
    std::os::unix::fs::symlink(&outside, f.folder.join("Album/link.flac")).unwrap();
    std::os::unix::fs::symlink(&f.temp_path, f.folder.join("Up")).unwrap();
    publish_tracks(
        &f,
        &[
            ("L", "Album/link.flac", false),
            ("D", "Up/outside.flac", false),
        ],
        &guest,
    )
    .await;
    assert_eq!(
        get(&f, "/rpm/track/L", guest_reader(&f, &guest)).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&f, "/rpm/track/D", guest_reader(&f, &guest)).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn track_found_when_case_differs() {
    let f = fixture();
    let guest = Identity::generate();
    write(&f.folder, "artist/album/01 song.flac", b"music");
    publish_tracks(&f, &[("A", "Artist/Album/01 Song.flac", false)], &guest).await;
    let (status, _, body) = get(&f, "/rpm/track/A", guest_reader(&f, &guest)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, &b"music"[..]);
}

#[tokio::test]
async fn track_found_when_name_is_nfd_on_disk() {
    let f = fixture();
    let guest = Identity::generate();
    write(&f.folder, "Cafe\u{301}/Ole\u{301}.flac", b"music");
    publish_tracks(&f, &[("A", "Caf\u{e9}/Ol\u{e9}.flac", false)], &guest).await;
    let (status, _, body) = get(&f, "/rpm/track/A", guest_reader(&f, &guest)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, &b"music"[..]);
}

#[tokio::test]
async fn interrupted_publish_keeps_previous_catalogue() {
    let f = fixture();
    let guest = Identity::generate();
    publish_tracks(&f, &[("A", "a.flac", false)], &guest).await;
    let full = upload(&catalogue("V2", &[]), &catalogue("G2", &[]), &[]);
    let cut = full[..full.len() / 2].to_vec();
    assert_eq!(publish(&f, &f.owner, cut).await, StatusCode::BAD_REQUEST);
    let (_, _, body) = get(&f, "/rpm/catalog", guest_reader(&f, &guest)).await;
    assert!(String::from_utf8_lossy(&body).contains(r#""version":"G1""#));
    assert_eq!(f.library.track_count(), 1);
}

#[tokio::test]
async fn restart_keeps_device_id_and_catalogue() {
    let f = fixture();
    let guest = Identity::generate();
    publish_tracks(&f, &[("A", "a.flac", false)], &guest).await;
    let id = f.library.identity().device_id().to_string();
    let reopened = Library::open(&f.folder, &f.data, f.space.clone()).unwrap();
    assert_eq!(reopened.identity().device_id(), id);
    assert_eq!(reopened.kind_of(guest.public_key()), Some(PeerKind::Guest));
    let (status, _, body) = call(
        &reopened,
        request(Method::GET, "/rpm/catalog", as_reader(&reopened, &guest)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(String::from_utf8_lossy(&body).contains(r#""version":"G1""#));
}

#[tokio::test]
async fn claim_change_is_read_without_restart() {
    let f = fixture();
    let newcomer = Identity::generate();
    assert!(!f.library.is_owner(newcomer.public_key()));
    // A different size, so the change is seen even when the modified time does not move.
    claim(&f.folder, &[&f.owner, &newcomer], "Chivan and more");
    assert!(f.library.is_owner(newcomer.public_key()));
    assert_eq!(f.library.display_name(), "Chivan and more");
}

#[test]
fn folder_without_a_library_id_is_not_opened() {
    let temp = tempfile::tempdir().unwrap();
    let folder = temp.path().join("NotALibrary");
    fs::create_dir_all(&folder).unwrap();
    let space = Arc::new(Space::new(temp.path().into(), String::new(), None));
    assert!(Library::open(&folder, &temp.path().join("data"), space).is_err());
}

#[test]
fn hello_reports_library_id_name_and_space() {
    let f = fixture();
    let hello = f.library.hello();
    assert_eq!(hello.library_id.as_deref(), Some(LIBRARY_ID));
    assert_eq!(hello.name, "Chivan");
    assert_eq!(hello.device_id, f.library.identity().device_id());
    let nas = hello.nas.unwrap();
    assert_eq!(nas.limit_bytes, Some(1000));
    assert_eq!(nas.smb, "smb://nas/music/rpm");
}

#[test]
fn index_of_20000_tracks_builds_in_under_one_second() {
    let rows: Vec<String> = (0..20_000)
        .map(|i| {
            format!(
                r#"{{"rpmId":"RPM{i}","path":"Artist {}/Album/{i}.flac","title":"Song {i}"}}"#,
                i / 12
            )
        })
        .collect();
    let json = format!(r#"{{"version":"v","tracks":[{}]}}"#, rows.join(","));
    let started = Instant::now();
    let index = Index::build(&CatalogView::parse(json.as_bytes()).unwrap());
    assert_eq!(index.len(), 20_000);
    assert!(started.elapsed().as_secs_f64() < 1.0);
    assert_eq!(index.siblings("RPM0").len(), 11);
}
