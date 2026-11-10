mod common;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use common::get;
use rpm_daemon::config::{load, parse_size, Config};
use rpm_daemon::supervisor::{library_folders, Supervisor};
use rpm_wire::PeerHello;

fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    move |name: &str| map.get(name).cloned()
}

fn make_library(root: &Path, folder: &str, id: &str) -> PathBuf {
    let path = root.join(folder);
    fs::create_dir_all(path.join(".rpm")).unwrap();
    fs::write(
        path.join(".rpm/library.json"),
        format!("{{\"id\":\"{id}\"}}\n"),
    )
    .unwrap();
    path
}

const ID_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ID_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[test]
fn config_reads_toml_and_env_wins() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("music");
    fs::create_dir_all(&root).unwrap();
    let file = temp.path().join("config.toml");
    fs::write(
        &file,
        format!(
            "root = {:?}\nlimit = \"2TB\"\nname = \"DiskStation\"\ndata = {:?}\nsmb_share = \"smb://ds/music\"\n",
            root,
            temp.path().join("data")
        ),
    )
    .unwrap();
    let config = load(Some(&file), &env(&[])).unwrap();
    assert_eq!(config.limit, Some(2_000_000_000_000));
    assert_eq!(config.name, "DiskStation");
    assert_eq!(config.smb_share, "smb://ds/music");
    assert_eq!(config.port, None);

    let config = load(
        Some(&file),
        &env(&[
            ("RPM_NAME", "Attic"),
            ("RPM_LIMIT", "500GiB"),
            ("RPM_PORT", "47100"),
        ]),
    )
    .unwrap();
    assert_eq!(config.name, "Attic");
    assert_eq!(config.limit, Some(500 * 1024 * 1024 * 1024));
    assert_eq!(config.port, Some(47100));
}

#[test]
fn config_from_environment_only() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("music");
    fs::create_dir_all(&root).unwrap();
    let data = temp.path().join("data");
    let config = load(
        None,
        &env(&[
            ("RPM_ROOT", root.to_str().unwrap()),
            ("RPM_DATA", data.to_str().unwrap()),
            ("RPM_SMB_SHARE", "smb://nas/music"),
            ("RPM_LIMIT", ""),
        ]),
    )
    .unwrap();
    assert_eq!(config.limit, None);
    assert!(!config.name.is_empty());
    assert!(data.is_dir());
}

#[test]
fn config_refuses_data_inside_root() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("music");
    fs::create_dir_all(&root).unwrap();
    let error = load(
        None,
        &env(&[
            ("RPM_ROOT", root.to_str().unwrap()),
            ("RPM_DATA", root.join("data").to_str().unwrap()),
            ("RPM_SMB_SHARE", "smb://nas/music"),
        ]),
    )
    .unwrap_err();
    assert!(error.0.contains("inside root"));
}

#[test]
fn config_needs_root_data_and_smb_share() {
    let temp = tempfile::tempdir().unwrap();
    assert!(load(None, &env(&[])).unwrap_err().0.contains("root"));
    let missing = load(
        None,
        &env(&[
            ("RPM_ROOT", temp.path().to_str().unwrap()),
            ("RPM_DATA", "/nonexistent-but-unused"),
        ]),
    )
    .unwrap_err();
    assert!(missing.0.contains("smb_share"));
}

#[test]
fn sizes_parse_in_both_unit_families() {
    assert_eq!(parse_size("2TB"), Some(Some(2_000_000_000_000)));
    assert_eq!(parse_size("1.5 TB"), Some(Some(1_500_000_000_000)));
    assert_eq!(parse_size("100GiB"), Some(Some(100 * 1024 * 1024 * 1024)));
    assert_eq!(parse_size("12345"), Some(Some(12345)));
    assert_eq!(parse_size(""), Some(None));
    assert_eq!(parse_size("lots"), None);
    assert_eq!(parse_size("2 parsecs"), None);
}

#[test]
fn only_folders_with_a_library_id_are_libraries() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    make_library(root, "Chivan", ID_A);
    make_library(root, "@eaDir", ID_B);
    make_library(root, ".hidden", ID_B);
    fs::create_dir_all(root.join("Photos")).unwrap();
    let bad = root.join("Broken");
    fs::create_dir_all(bad.join(".rpm")).unwrap();
    fs::write(bad.join(".rpm/library.json"), "{\"id\":\"short\"}").unwrap();
    let names: Vec<String> = library_folders(root)
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["Chivan"]);
}

fn config(temp: &Path) -> Config {
    let root = temp.join("music");
    fs::create_dir_all(&root).unwrap();
    load(
        None,
        &env(&[
            ("RPM_ROOT", root.to_str().unwrap()),
            ("RPM_DATA", temp.join("data").to_str().unwrap()),
            ("RPM_SMB_SHARE", "smb://nas/music/rpm"),
            ("RPM_NAME", "TestNAS"),
            ("RPM_LIMIT", "1MB"),
        ]),
    )
    .unwrap()
}

#[tokio::test]
async fn new_library_folder_is_served_after_a_rescan() {
    let temp = tempfile::tempdir().unwrap();
    let config = config(temp.path());
    let root = config.root.clone();
    let supervisor = Supervisor::new(config, None).unwrap();
    supervisor.start().await.unwrap();
    assert!(supervisor.served().is_empty());

    make_library(&root, "Chivan", ID_A);
    supervisor.rescan().await;
    let served = supervisor.served();
    assert_eq!(served.len(), 1);
    assert_eq!(served[0].library_id, ID_A);

    let reply = get(served[0].port, "/rpm/hello", None).await;
    let hello: PeerHello = serde_json::from_slice(&reply.body).unwrap();
    assert_eq!(hello.library_id.as_deref(), Some(ID_A));
    supervisor.shutdown();
}

#[tokio::test]
async fn removed_library_folder_stops_being_served() {
    let temp = tempfile::tempdir().unwrap();
    let config = config(temp.path());
    let root = config.root.clone();
    let folder = make_library(&root, "Chivan", ID_A);
    let supervisor = Supervisor::new(config, None).unwrap();
    supervisor.start().await.unwrap();
    let port = supervisor.served()[0].port;

    fs::remove_dir_all(&folder).unwrap();
    supervisor.rescan().await;
    assert!(supervisor.served().is_empty());
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .is_err());
    supervisor.shutdown();
}

#[tokio::test]
async fn nas_hello_lists_libraries_with_claim_names_and_accounts() {
    let temp = tempfile::tempdir().unwrap();
    let config = config(temp.path());
    let root = config.root.clone();
    let a = make_library(&root, "Chivan", ID_A);
    make_library(&root, "Gianni", ID_B);
    fs::write(
        a.join(".rpm/daemon-owner.json"),
        r#"{"keys":[{"publicKey":"AAAA","name":"Chivan's music","account":"fp-1"}]}"#,
    )
    .unwrap();
    let supervisor = Supervisor::new(config, None).unwrap();
    let port = supervisor.start().await.unwrap();

    let reply = get(port, "/rpm/hello", None).await;
    let hello: PeerHello = serde_json::from_slice(&reply.body).unwrap();
    assert_eq!(hello.name, "TestNAS");
    assert!(hello.library_id.is_none());
    let nas = hello.nas.unwrap();
    assert_eq!(nas.libraries.len(), 2);
    assert_eq!(nas.libraries[0].folder, "Chivan");
    assert_eq!(nas.libraries[0].name, "Chivan's music");
    assert_eq!(nas.libraries[0].account, "fp-1");
    assert_eq!(nas.libraries[1].name, "Gianni");
    assert_eq!(nas.libraries[1].account, "");
    assert!(!nas.libraries[1].device_id.is_empty());
    supervisor.shutdown();
}

#[tokio::test]
async fn nas_hello_reports_limit_used_and_free() {
    let temp = tempfile::tempdir().unwrap();
    let config = config(temp.path());
    let root = config.root.clone();
    let folder = make_library(&root, "Chivan", ID_A);
    fs::write(folder.join("song.flac"), vec![0u8; 5000]).unwrap();
    let supervisor = Supervisor::new(config, None).unwrap();
    let port = supervisor.start().await.unwrap();

    let reply = get(port, "/rpm/hello", None).await;
    let nas = serde_json::from_slice::<PeerHello>(&reply.body)
        .unwrap()
        .nas
        .unwrap();
    assert_eq!(nas.limit_bytes, Some(1_000_000));
    assert!(nas.used_bytes >= 5000);
    assert!(nas.free_bytes > 0);
    assert_eq!(nas.smb, "smb://nas/music/rpm");
    supervisor.shutdown();
}

#[test]
fn a_root_with_a_library_id_is_the_only_library() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("music");
    make_library(&root, "", ID_A);
    // An artist folder that happens to hold an id is part of the root's library, not a library.
    make_library(&root, "Artist", ID_B);
    assert_eq!(library_folders(&root), vec![root.canonicalize().unwrap()]);
}

#[tokio::test]
async fn existing_music_is_reported_and_then_served_as_the_root_library() {
    let temp = tempfile::tempdir().unwrap();
    let config = config(temp.path());
    let root = config.root.clone();
    let song = root.join("Artist/Album/01 Song.flac");
    fs::create_dir_all(song.parent().unwrap()).unwrap();
    fs::write(&song, vec![1u8; 100]).unwrap();
    let supervisor = Supervisor::new(config, None).unwrap();
    let port = supervisor.start().await.unwrap();

    let hello = |body: &[u8]| {
        serde_json::from_slice::<PeerHello>(body)
            .unwrap()
            .nas
            .unwrap()
    };
    let nas = hello(&get(port, "/rpm/hello", None).await.body);
    assert!(nas.existing_music);
    assert!(nas.libraries.is_empty());

    // A computer makes the folder itself a library: it adds only .rpm/library.json.
    make_library(&root, "", ID_A);
    supervisor.rescan().await;

    let nas = hello(&get(port, "/rpm/hello", None).await.body);
    assert!(!nas.existing_music);
    assert_eq!(nas.libraries.len(), 1);
    assert_eq!(nas.libraries[0].folder, "");
    assert_eq!(nas.libraries[0].library_id, ID_A);
    assert_eq!(supervisor.served().len(), 1);
    assert_eq!(fs::read(&song).unwrap(), vec![1u8; 100]);
    supervisor.shutdown();
}

#[tokio::test]
async fn nas_peer_gives_no_session() {
    let temp = tempfile::tempdir().unwrap();
    let supervisor = Supervisor::new(config(temp.path()), None).unwrap();
    let port = supervisor.start().await.unwrap();
    assert_eq!(get(port, "/rpm/catalog", Some("x")).await.status, 401);
    supervisor.shutdown();
}

#[tokio::test]
async fn restart_keeps_the_nas_device_id() {
    let temp = tempfile::tempdir().unwrap();
    let first = Supervisor::new(config(temp.path()), None).unwrap();
    let port = first.start().await.unwrap();
    let before: PeerHello =
        serde_json::from_slice(&get(port, "/rpm/hello", None).await.body).unwrap();
    first.shutdown();
    let second = Supervisor::new(config(temp.path()), None).unwrap();
    let port = second.start().await.unwrap();
    let after: PeerHello =
        serde_json::from_slice(&get(port, "/rpm/hello", None).await.body).unwrap();
    assert_eq!(before.device_id, after.device_id);
    second.shutdown();
}
