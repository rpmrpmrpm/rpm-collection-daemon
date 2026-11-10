//! One library folder: its id, its key, the uploads, and the routes it answers.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use bytes::Bytes;
use hyper::header::RANGE;
use hyper::{Method, Response, StatusCode};
use rpm_peer::http::{bytes_response, status, tail, AuthedRequest, Body, Routes};
use rpm_peer::identity::{id_for, Identity};
use rpm_peer::trust::{Reader, Trust};
use rpm_wire::base::{from_base64url, to_base64url};
use rpm_wire::routes::{self, WIRE_VERSION};
use rpm_wire::{CatalogView, NasLibraryEntry, NasPublish, PeerHello, PeerKind};

use crate::claim::{ClaimFile, ClaimWatch};
use crate::index::Index;
use crate::space::Space;
use crate::{covers, media, store};

/// The library id file's path inside the library folder, as `MirrorLocation` in `RPM.Core` writes it.
pub const ID_PATH: &str = ".rpm/library.json";

/// The largest picture the owner's computer may upload.
pub const MAX_ART_BYTES: usize = 4 * 1024 * 1024;

/// A picture asked for by key is the same picture every time.
const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// The latest upload, ready to serve.
pub struct Snapshot {
    pub owner: Bytes,
    pub guest: Bytes,
    pub owner_version: String,
    pub guest_version: String,
    pub index: Index,
    pub at_ms: u64,
}

/// Each key an owner computer trusts, with how it knows it.
type TrustList = Vec<(Vec<u8>, PeerKind)>;

#[derive(Default)]
struct State {
    snapshot: Option<Arc<Snapshot>>,
    /// Each owner computer's trust list, by its device id.
    trust: HashMap<String, TrustList>,
}

pub struct Library {
    folder: PathBuf,
    folder_name: String,
    library_id: String,
    identity: Identity,
    data: PathBuf,
    space: Arc<Space>,
    claim: ClaimWatch,
    state: RwLock<State>,
}

#[derive(Debug)]
pub enum PublishError {
    /// The body is not a valid upload. Nothing changed.
    Invalid,
    /// The upload could not be saved. Nothing changed.
    Io(io::Error),
}

impl Library {
    /// Opens the library in `folder`. Its key and uploads are kept in `<data>/<library id>`.
    ///
    /// Fails when the folder has no valid library id: a folder rpm has not set up is not a library.
    pub fn open(folder: &Path, data: &Path, space: Arc<Space>) -> io::Result<Arc<Library>> {
        let folder = folder.canonicalize()?;
        let library_id = read_library_id(&folder)?;
        let data = data.join(&library_id);
        let identity = Identity::load_or_create(&data.join("peer.key"))?;
        let folder_name = folder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let library = Library {
            claim: ClaimWatch::new(folder.clone()),
            folder,
            folder_name,
            library_id,
            identity,
            data,
            space,
            state: RwLock::new(State::default()),
        };
        library.load();
        Ok(Arc::new(library))
    }

    fn load(&self) {
        let mut stored = store::load_all(&self.uploads());
        stored.sort_by_key(|s| s.at_ms);
        for upload in stored {
            if let Err(error) = self.apply(&upload.publisher, upload.at_ms, &upload.body) {
                log::warn!(
                    "library {}: a stored upload could not be read: {error:?}",
                    self.library_id
                );
            }
        }
    }

    pub fn library_id(&self) -> &str {
        &self.library_id
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    pub fn folder_name(&self) -> &str {
        &self.folder_name
    }

    fn uploads(&self) -> PathBuf {
        self.data.join("uploads")
    }

    fn art_dir(&self) -> PathBuf {
        self.data.join("art")
    }

    fn claim(&self) -> ClaimFile {
        self.claim.current().unwrap_or_default()
    }

    /// The name the owner gave the library, or the folder name before it is claimed.
    pub fn display_name(&self) -> String {
        self.claim()
            .name()
            .map(str::to_string)
            .unwrap_or_else(|| self.folder_name.clone())
    }

    /// This library as the NAS's hello lists it.
    pub fn entry(&self) -> NasLibraryEntry {
        let claim = self.claim();
        NasLibraryEntry {
            folder: self.folder_name.clone(),
            library_id: self.library_id.clone(),
            name: claim
                .name()
                .map(str::to_string)
                .unwrap_or_else(|| self.folder_name.clone()),
            account: claim.account().unwrap_or_default().to_string(),
            device_id: self.identity.device_id().to_string(),
            public_key: to_base64url(self.identity.public_key()),
        }
    }

    pub fn snapshot(&self) -> Option<Arc<Snapshot>> {
        self.state.read().unwrap().snapshot.clone()
    }

    pub fn track_count(&self) -> usize {
        self.snapshot().map(|s| s.index.len()).unwrap_or(0)
    }

    /// Takes an upload from the owner computer with `publisher_key`. It is saved before it is served,
    /// so a restart right after the reply still has it.
    pub fn publish(&self, publisher_key: &[u8], body: &[u8]) -> Result<(), PublishError> {
        let publisher = id_for(publisher_key);
        let at_ms = now_ms();
        // Check it before saving, so a bad upload never replaces a good one on the disk.
        parse(body).ok_or(PublishError::Invalid)?;
        store::save(&self.uploads(), &publisher, at_ms, body).map_err(PublishError::Io)?;
        self.apply(&publisher, at_ms, body)
    }

    fn apply(&self, publisher: &str, at_ms: u64, body: &[u8]) -> Result<(), PublishError> {
        let (snapshot, trust) = parse(body).ok_or(PublishError::Invalid)?;
        let mut state = self.state.write().unwrap();
        state.trust.insert(publisher.to_string(), trust);
        let newer = state
            .snapshot
            .as_ref()
            .map(|current| at_ms >= current.at_ms)
            .unwrap_or(true);
        if newer {
            state.snapshot = Some(Arc::new(Snapshot { at_ms, ..snapshot }));
        }
        Ok(())
    }

    /// Saves a picture the owner computer uploaded, under a key such as `pl:12`.
    pub fn save_art(&self, key: &str, bytes: &[u8]) -> io::Result<()> {
        let name = art_file_name(key)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "bad picture key"))?;
        store::write_atomic(&self.art_dir().join(name), bytes)
    }

    pub fn art(&self, key: &str) -> Option<Vec<u8>> {
        fs::read(self.art_dir().join(art_file_name(key)?)).ok()
    }

    async fn catalog(&self, request: &AuthedRequest, head_only: bool) -> Response<Body> {
        let Some(snapshot) = self.snapshot() else {
            // Nothing has been uploaded yet. The reader keeps what it has and asks again later.
            return status(StatusCode::SERVICE_UNAVAILABLE);
        };
        let (bytes, version) = match request.reader.kind {
            PeerKind::Account => (&snapshot.owner, &snapshot.owner_version),
            PeerKind::Guest => (&snapshot.guest, &snapshot.guest_version),
        };
        if request.query.get("have") == Some(version) {
            return status(StatusCode::NO_CONTENT);
        }
        bytes_response(
            StatusCode::OK,
            "application/json; charset=utf-8",
            bytes.clone(),
            head_only,
            None,
        )
    }

    async fn track(
        &self,
        rpm_id: String,
        request: &AuthedRequest,
        head_only: bool,
    ) -> Response<Body> {
        let Some(snapshot) = self.snapshot() else {
            return status(StatusCode::NOT_FOUND);
        };
        let folder = self.folder.clone();
        let found = tokio::task::spawn_blocking(move || snapshot.index.resolve(&folder, &rpm_id))
            .await
            .ok()
            .flatten();
        let Some(path) = found else {
            return status(StatusCode::NOT_FOUND);
        };
        let range = request
            .headers
            .get(RANGE)
            .and_then(|value| value.to_str().ok());
        media::serve_file(&path, range, head_only).await
    }

    async fn picture(&self, rpm_id: String, disc: bool, head_only: bool) -> Response<Body> {
        let Some(snapshot) = self.snapshot() else {
            return status(StatusCode::NOT_FOUND);
        };
        let folder = self.folder.clone();
        let found = tokio::task::spawn_blocking(move || {
            let file = snapshot.index.resolve(&folder, &rpm_id)?;
            let album = file.parent()?.to_path_buf();
            // A kept library's pictures chosen in rpm come before the folder's own.
            if disc {
                return match covers::chosen_disc(&folder, &album) {
                    covers::Chosen::Picture(chosen) => fs::read(chosen).ok(),
                    covers::Chosen::Removed => None,
                    covers::Chosen::Nothing => fs::read(covers::disc_image(&album)?).ok(),
                };
            }
            let folder_picture = match covers::chosen_cover(&folder, &album) {
                covers::Chosen::Picture(chosen) => Some(chosen),
                covers::Chosen::Removed => None,
                covers::Chosen::Nothing => covers::folder_image(&album),
            };
            if let Some(image) = folder_picture {
                if let Ok(bytes) = fs::read(image) {
                    return Some(bytes);
                }
            }
            if let Some(bytes) = covers::embedded(&file) {
                return Some(bytes);
            }
            // Another track of the album may carry the picture when this one does not.
            snapshot
                .index
                .siblings(&rpm_id)
                .iter()
                .filter_map(|path| crate::index::resolve_path(&folder, path))
                .find_map(|sibling| covers::embedded(&sibling))
        })
        .await
        .ok()
        .flatten()
        .filter(|bytes| !bytes.is_empty());
        match found {
            Some(bytes) => image(bytes, head_only),
            None => status(StatusCode::NOT_FOUND),
        }
    }
}

fn image(bytes: Vec<u8>, head_only: bool) -> Response<Body> {
    let kind = covers::sniff(&bytes);
    bytes_response(
        StatusCode::OK,
        kind,
        bytes.into(),
        head_only,
        Some(IMMUTABLE),
    )
}

/// Reads an upload into a snapshot and a trust list. `None` when it is not a valid upload.
fn parse(body: &[u8]) -> Option<(Snapshot, TrustList)> {
    let publish: NasPublish = serde_json::from_slice(body).ok()?;
    let owner = CatalogView::parse(publish.owner.get().as_bytes()).ok()?;
    let guest = CatalogView::parse(publish.guest.get().as_bytes()).ok()?;
    let trust = publish
        .trust
        .iter()
        .filter_map(|t| Some((from_base64url(&t.public_key)?, t.kind)))
        .collect();
    let snapshot = Snapshot {
        index: Index::build(&owner),
        owner: Bytes::copy_from_slice(publish.owner.get().as_bytes()),
        guest: Bytes::copy_from_slice(publish.guest.get().as_bytes()),
        owner_version: owner.version,
        guest_version: guest.version,
        at_ms: 0,
    };
    Some((snapshot, trust))
}

/// The file name for a picture key: letters, digits, `:`, `-` and `_` only, at most 64 characters.
fn art_file_name(key: &str) -> Option<String> {
    let valid = !key.is_empty()
        && key.len() <= 64
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '-' | '_'));
    valid.then(|| format!("{}.img", key.replace(':', "_")))
}

/// Reads `.rpm/library.json`: `{"id":"<32 lowercase hex>"}`.
pub fn read_library_id(folder: &Path) -> io::Result<String> {
    let text = fs::read_to_string(folder.join(ID_PATH))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let id = value
        .get("id")
        .and_then(|id| id.as_str())
        .filter(|id| {
            id.len() == 32
                && id
                    .chars()
                    .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no library id"))?;
    Ok(id.to_string())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Trust for Library {
    fn kind_of(&self, public_key: &[u8]) -> Option<PeerKind> {
        let state = self.state.read().unwrap();
        let mut found = None;
        for list in state.trust.values() {
            for (key, kind) in list {
                if key == public_key {
                    // A key that one owner computer knows as the owner's own device and another
                    // knows as a guest reads as the owner's own.
                    if *kind == PeerKind::Account {
                        return Some(PeerKind::Account);
                    }
                    found = Some(*kind);
                }
            }
        }
        found
    }

    fn is_owner(&self, public_key: &[u8]) -> bool {
        self.claim().holds(public_key)
    }
}

#[async_trait]
impl Routes for Library {
    fn hello(&self) -> PeerHello {
        PeerHello {
            version: WIRE_VERSION,
            device_id: self.identity.device_id().to_string(),
            name: self.display_name(),
            public_key: to_base64url(self.identity.public_key()),
            track_count: self.track_count() as i32,
            library_id: Some(self.library_id.clone()),
            nas: Some(self.space.report(Vec::new(), false)),
        }
    }

    async fn handle(&self, request: AuthedRequest) -> Response<Body> {
        let read = matches!(request.method, Method::GET | Method::HEAD);
        let head_only = request.method == Method::HEAD;
        let path = request.path.clone();

        if path == routes::CATALOG {
            return if read {
                self.catalog(&request, head_only).await
            } else {
                status(StatusCode::METHOD_NOT_ALLOWED)
            };
        }

        if path == routes::NAS_PUBLISH {
            if request.method != Method::POST {
                return status(StatusCode::METHOD_NOT_ALLOWED);
            }
            return publish_answer(self, &request.reader, &request.body);
        }

        if let Some(key) = tail(&path, routes::NAS_ART) {
            if request.method != Method::PUT {
                return status(StatusCode::METHOD_NOT_ALLOWED);
            }
            if !request.reader.owner {
                return status(StatusCode::FORBIDDEN);
            }
            if request.body.len() > MAX_ART_BYTES {
                return status(StatusCode::PAYLOAD_TOO_LARGE);
            }
            return match self.save_art(&key, &request.body) {
                Ok(()) => status(StatusCode::NO_CONTENT),
                Err(error) if error.kind() == io::ErrorKind::InvalidInput => {
                    status(StatusCode::BAD_REQUEST)
                }
                Err(_) => status(StatusCode::INTERNAL_SERVER_ERROR),
            };
        }

        if !read {
            return status(StatusCode::METHOD_NOT_ALLOWED);
        }
        if let Some(id) = tail(&path, routes::TRACK) {
            return self.track(id, &request, head_only).await;
        }
        if let Some(id) = tail(&path, routes::COVER) {
            return self.picture(id, false, head_only).await;
        }
        if let Some(id) = tail(&path, routes::DISC) {
            return self.picture(id, true, head_only).await;
        }
        if let Some(key) = tail(&path, routes::ART) {
            return match self.art(&key) {
                Some(bytes) if !bytes.is_empty() => image(bytes, head_only),
                _ => status(StatusCode::NOT_FOUND),
            };
        }
        status(StatusCode::NOT_FOUND)
    }
}

fn publish_answer(library: &Library, reader: &Reader, body: &[u8]) -> Response<Body> {
    if !reader.owner {
        return status(StatusCode::FORBIDDEN);
    }
    match library.publish(&reader.public_key, body) {
        Ok(()) => status(StatusCode::NO_CONTENT),
        Err(PublishError::Invalid) => status(StatusCode::BAD_REQUEST),
        Err(PublishError::Io(error)) => {
            log::warn!(
                "library {}: an upload could not be saved: {error}",
                library.library_id
            );
            status(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
