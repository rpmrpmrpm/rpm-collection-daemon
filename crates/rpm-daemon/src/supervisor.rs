//! Runs the NAS's own peer and one peer per library folder under `root`, and follows folders that are
//! added or removed.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use hyper::{Response, StatusCode};
use rpm_collection::library::{read_library_id, ID_PATH};
use rpm_collection::{Library, Space};
use rpm_peer::beacon::{Announcement, Announcer};
use rpm_peer::certificate::server_config;
use rpm_peer::http::{serve, status, AuthedRequest, Body, Limits, Routes};
use rpm_peer::identity::Identity;
use rpm_peer::sessions::Sessions;
use rpm_peer::trust::Trust;
use rpm_wire::base::to_base64url;
use rpm_wire::routes::WIRE_VERSION;
use rpm_wire::{txt, PeerHello, PeerKind};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::config::Config;

/// How often `root` is scanned for library folders.
pub const RESCAN: Duration = Duration::from_secs(10);
/// How often the space used is measured, besides after each upload.
pub const MEASURE: Duration = Duration::from_secs(10 * 60);
/// How many ports after the first one the libraries may use when a fixed port is set.
const PORT_RANGE: u16 = 64;

/// A library folder being served.
struct Running {
    library: Arc<Library>,
    port: u16,
    stop: CancellationToken,
    announced_name: String,
    announcement: Option<Announcement>,
    /// The arrival time of the upload the space was last measured after.
    measured_upload: u64,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

/// A library being served, as the supervisor reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Served {
    pub folder: String,
    pub library_id: String,
    pub port: u16,
}

pub struct Supervisor {
    config: Config,
    announcer: Option<Announcer>,
    space: Arc<Space>,
    nas: Identity,
    running: Mutex<HashMap<PathBuf, Running>>,
    nas_port: Mutex<Option<u16>>,
    nas_announcement: Mutex<Option<Announcement>>,
    stop: CancellationToken,
}

impl Supervisor {
    /// A supervisor for `config`. `announcer` is `None` where multicast DNS is not wanted, as in tests.
    pub fn new(config: Config, announcer: Option<Announcer>) -> io::Result<Arc<Supervisor>> {
        let nas = Identity::load_or_create(&config.data.join("nas.key"))?;
        let space = Arc::new(Space::new(
            config.root.clone(),
            config.smb_share.clone(),
            config.limit,
        ));
        Ok(Arc::new(Supervisor {
            config,
            announcer,
            space,
            nas,
            running: Mutex::new(HashMap::new()),
            nas_port: Mutex::new(None),
            nas_announcement: Mutex::new(None),
            stop: CancellationToken::new(),
        }))
    }

    /// Starts the NAS's own peer, measures the space and serves the libraries found now. Returns the
    /// NAS peer's port.
    pub async fn start(self: &Arc<Self>) -> io::Result<u16> {
        self.refresh_space().await;
        let listener = self.bind(self.config.port).await?;
        let port = listener.local_addr()?.port();
        let tls = server_config(&self.nas).map_err(io::Error::other)?;
        let routes: Arc<NasRoutes> = Arc::new(NasRoutes {
            supervisor: Arc::downgrade(self),
        });
        tokio::spawn(serve(
            listener,
            tls,
            Arc::new(Sessions::new(self.nas.device_id())),
            Arc::new(NoReaders),
            routes,
            Limits::default(),
            self.stop.clone(),
        ));
        *self.nas_port.lock().unwrap() = Some(port);
        if let Some(announcer) = &self.announcer {
            let record = vec![
                (txt::VERSION, WIRE_VERSION.to_string()),
                (txt::DEVICE_ID, self.nas.device_id().to_string()),
                (txt::NAME, self.config.name.clone()),
                (txt::PUBLIC_KEY, to_base64url(self.nas.public_key())),
                (txt::ROLE, txt::ROLE_NAS.to_string()),
                (txt::SMB, self.config.smb_share.clone()),
            ];
            match announcer.announce(&self.config.name, port, &record) {
                Ok(announcement) => *self.nas_announcement.lock().unwrap() = Some(announcement),
                Err(error) => log::warn!("the NAS could not be announced: {error}"),
            }
        }
        log::info!(
            "{} ({}) listening on port {port}",
            self.config.name,
            self.nas.device_id()
        );
        self.rescan().await;
        Ok(port)
    }

    /// Scans until `stop` is cancelled.
    pub async fn run(self: Arc<Self>, stop: CancellationToken) {
        let mut rescan = tokio::time::interval(RESCAN);
        let mut measure = tokio::time::interval(MEASURE);
        rescan.tick().await;
        measure.tick().await;
        loop {
            tokio::select! {
                _ = stop.cancelled() => break,
                _ = rescan.tick() => self.rescan().await,
                _ = measure.tick() => self.refresh_space().await,
            }
        }
        self.shutdown();
    }

    /// Stops every peer and withdraws every announcement.
    pub fn shutdown(&self) {
        self.stop.cancel();
        self.running.lock().unwrap().clear();
        self.nas_announcement.lock().unwrap().take();
    }

    /// The libraries being served.
    pub fn served(&self) -> Vec<Served> {
        let mut served: Vec<Served> = self
            .running
            .lock()
            .unwrap()
            .values()
            .map(|r| Served {
                folder: r.library.folder_name().to_string(),
                library_id: r.library.library_id().to_string(),
                port: r.port,
            })
            .collect();
        served.sort_by(|a, b| a.folder.cmp(&b.folder));
        served
    }

    /// Measures the space used and free.
    pub async fn refresh_space(&self) {
        let space = self.space.clone();
        let _ = tokio::task::spawn_blocking(move || space.refresh()).await;
    }

    /// Starts serving library folders that appeared, stops those that went, announces a name the
    /// owner changed, and measures the space after an upload.
    pub async fn rescan(&self) {
        let found = library_folders(&self.config.root);
        let gone: Vec<PathBuf> = {
            let running = self.running.lock().unwrap();
            running
                .keys()
                .filter(|folder| !found.contains(folder))
                .cloned()
                .collect()
        };
        for folder in gone {
            log::info!(
                "{} is no longer a library; it is not served",
                folder.display()
            );
            self.running.lock().unwrap().remove(&folder);
        }

        let new: Vec<PathBuf> = {
            let running = self.running.lock().unwrap();
            found
                .iter()
                .filter(|folder| !running.contains_key(*folder))
                .cloned()
                .collect()
        };
        for folder in new {
            match self.start_library(&folder).await {
                Ok(running) => {
                    log::info!(
                        "serving {} ({}) on port {}",
                        folder.display(),
                        running.library.library_id(),
                        running.port
                    );
                    self.running.lock().unwrap().insert(folder, running);
                }
                Err(error) => log::warn!("{} could not be served: {error}", folder.display()),
            }
        }

        let mut uploaded = false;
        {
            let mut running = self.running.lock().unwrap();
            for entry in running.values_mut() {
                let name = entry.library.display_name();
                if name != entry.announced_name {
                    entry.announcement = None;
                    entry.announcement = self.announce_library(&entry.library, entry.port);
                    entry.announced_name = name;
                }
                let latest = entry.library.snapshot().map(|s| s.at_ms).unwrap_or(0);
                if latest != entry.measured_upload {
                    entry.measured_upload = latest;
                    uploaded = true;
                }
            }
        }
        if uploaded {
            self.refresh_space().await;
        }
    }

    async fn start_library(&self, folder: &Path) -> io::Result<Running> {
        let library = Library::open(folder, &self.config.data, self.space.clone())?;
        let listener = self.bind_library().await?;
        let port = listener.local_addr()?.port();
        let tls = server_config(library.identity()).map_err(io::Error::other)?;
        let stop = self.stop.child_token();
        tokio::spawn(serve(
            listener,
            tls,
            Arc::new(Sessions::new(library.identity().device_id())),
            library.clone(),
            library.clone(),
            Limits::default(),
            stop.clone(),
        ));
        let announcement = self.announce_library(&library, port);
        Ok(Running {
            announced_name: library.display_name(),
            measured_upload: library.snapshot().map(|s| s.at_ms).unwrap_or(0),
            library,
            port,
            stop,
            announcement,
        })
    }

    fn announce_library(&self, library: &Library, port: u16) -> Option<Announcement> {
        let announcer = self.announcer.as_ref()?;
        let record = vec![
            (txt::VERSION, WIRE_VERSION.to_string()),
            (txt::DEVICE_ID, library.identity().device_id().to_string()),
            (txt::NAME, library.display_name()),
            (
                txt::PUBLIC_KEY,
                to_base64url(library.identity().public_key()),
            ),
            (txt::ROLE, txt::ROLE_LIBRARY.to_string()),
            (txt::LIBRARY_ID, library.library_id().to_string()),
        ];
        let instance = format!("{} on {}", library.folder_name(), self.config.name);
        match announcer.announce(&instance, port, &record) {
            Ok(announcement) => Some(announcement),
            Err(error) => {
                log::warn!("{instance} could not be announced: {error}");
                None
            }
        }
    }

    async fn bind(&self, port: Option<u16>) -> io::Result<TcpListener> {
        TcpListener::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port.unwrap_or(0)))).await
    }

    /// A listener for a library: the next free port after the NAS's when a fixed port is set, or any
    /// port.
    async fn bind_library(&self) -> io::Result<TcpListener> {
        let Some(first) = self.config.port else {
            return self.bind(None).await;
        };
        for offset in 1..=PORT_RANGE {
            if let Some(port) = first.checked_add(offset) {
                if let Ok(listener) = self.bind(Some(port)).await {
                    return Ok(listener);
                }
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            format!(
                "no free port from {} to {}",
                first + 1,
                first.saturating_add(PORT_RANGE)
            ),
        ))
    }

    fn nas_hello(&self) -> PeerHello {
        let mut libraries: Vec<_> = self
            .running
            .lock()
            .unwrap()
            .values()
            .map(|r| r.library.entry())
            .collect();
        libraries.sort_by(|a, b| a.folder.cmp(&b.folder));
        PeerHello {
            version: WIRE_VERSION,
            device_id: self.nas.device_id().to_string(),
            name: self.config.name.clone(),
            public_key: to_base64url(self.nas.public_key()),
            track_count: 0,
            library_id: None,
            nas: Some(self.space.report(libraries)),
        }
    }
}

/// The direct subfolders of `root` that hold a library id. Hidden folders and the folders NAS systems
/// keep for themselves (`@eaDir`, `#recycle`, `#snapshot`) are skipped.
pub fn library_folders(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut folders: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            !(name.starts_with('.') || name.starts_with('@') || name.starts_with('#'))
        })
        .map(|entry| entry.path())
        .filter(|folder| folder.join(ID_PATH).is_file() && read_library_id(folder).is_ok())
        .filter_map(|folder| folder.canonicalize().ok())
        .collect();
    folders.sort();
    folders
}

/// The NAS's own peer serves only its hello.
struct NasRoutes {
    supervisor: std::sync::Weak<Supervisor>,
}

#[async_trait]
impl Routes for NasRoutes {
    fn hello(&self) -> PeerHello {
        match self.supervisor.upgrade() {
            Some(supervisor) => supervisor.nas_hello(),
            None => PeerHello {
                version: WIRE_VERSION,
                device_id: String::new(),
                name: String::new(),
                public_key: String::new(),
                track_count: 0,
                library_id: None,
                nas: None,
            },
        }
    }

    async fn handle(&self, _: AuthedRequest) -> Response<Body> {
        status(StatusCode::NOT_FOUND)
    }
}

/// Nobody gets a session with the NAS's own peer. It has nothing to read but its hello.
struct NoReaders;

impl Trust for NoReaders {
    fn kind_of(&self, _: &[u8]) -> Option<PeerKind> {
        None
    }

    fn is_owner(&self, _: &[u8]) -> bool {
        false
    }
}
