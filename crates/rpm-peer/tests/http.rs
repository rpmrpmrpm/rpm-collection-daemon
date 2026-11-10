mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use common::{get, send, TestTrust};
use hyper::{Response, StatusCode};
use rpm_peer::certificate::server_config;
use rpm_peer::http::{bytes_response, serve, AuthedRequest, Body, Limits, Routes};
use rpm_peer::identity::Identity;
use rpm_peer::sessions::{handle, signed_bytes, ticket, Sessions};
use rpm_wire::base::to_base64url;
use rpm_wire::{PeerHello, PeerKind, SessionChallenge, SessionGrant, SessionProof};
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

struct EchoRoutes;

#[async_trait]
impl Routes for EchoRoutes {
    fn hello(&self) -> PeerHello {
        PeerHello {
            version: 4,
            device_id: "ID".into(),
            name: "Test".into(),
            public_key: "KEY".into(),
            track_count: 0,
            library_id: None,
            nas: None,
        }
    }

    async fn handle(&self, request: AuthedRequest) -> Response<Body> {
        let text = format!("{} {} {}", request.method, request.path, request.body.len());
        bytes_response(StatusCode::OK, "text/plain", text.into(), false, None)
    }
}

struct Peer {
    port: u16,
    identity: Identity,
    trust: Arc<TestTrust>,
    stop: CancellationToken,
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

async fn start(limits: Limits) -> Peer {
    let identity = Identity::generate();
    let tls = server_config(&identity).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let sessions = Arc::new(Sessions::new(identity.device_id()));
    let trust = Arc::new(TestTrust::default());
    let stop = CancellationToken::new();
    tokio::spawn(serve(
        listener,
        tls,
        sessions,
        trust.clone(),
        Arc::new(EchoRoutes),
        limits,
        stop.clone(),
    ));
    Peer {
        port,
        identity,
        trust,
        stop,
    }
}

async fn token(peer: &Peer, reader: &Identity) -> Option<String> {
    let reply = get(peer.port, "/rpm/session", None).await;
    let challenge: SessionChallenge = serde_json::from_slice(&reply.body).unwrap();
    let signature = reader.sign(&signed_bytes(
        peer.identity.device_id(),
        &challenge.challenge,
        "",
    ));
    let proof = SessionProof {
        public_key: to_base64url(reader.public_key()),
        challenge: challenge.challenge,
        signature: to_base64url(&signature),
        account: String::new(),
    };
    let body = serde_json::to_vec(&proof).unwrap();
    let reply = send(peer.port, "POST /rpm/session HTTP/1.1", &body).await;
    (reply.status == 200).then(|| {
        serde_json::from_slice::<SessionGrant>(&reply.body)
            .unwrap()
            .token
    })
}

#[tokio::test]
async fn hello_is_open_to_anyone() {
    let peer = start(Limits::default()).await;
    let reply = get(peer.port, "/rpm/hello", None).await;
    assert_eq!(reply.status, 200);
    let hello: PeerHello = serde_json::from_slice(&reply.body).unwrap();
    assert_eq!(hello.name, "Test");
}

#[tokio::test]
async fn catalog_without_token_is_401() {
    let peer = start(Limits::default()).await;
    assert_eq!(get(peer.port, "/rpm/catalog", None).await.status, 401);
    assert_eq!(
        get(peer.port, "/rpm/catalog", Some("made-up")).await.status,
        401
    );
}

#[tokio::test]
async fn trusted_reader_gets_through_with_a_token() {
    let peer = start(Limits::default()).await;
    let reader = Identity::generate();
    peer.trust.add(reader.public_key(), PeerKind::Guest);
    let token = token(&peer, &reader).await.unwrap();
    let reply = get(peer.port, "/rpm/catalog", Some(&token)).await;
    assert_eq!(reply.status, 200);
    assert_eq!(reply.body, b"GET /rpm/catalog 0");
}

#[tokio::test]
async fn untrusted_reader_gets_403_from_the_session_route() {
    let peer = start(Limits::default()).await;
    assert!(token(&peer, &Identity::generate()).await.is_none());
}

#[tokio::test]
async fn media_ticket_opens_its_track_without_a_header() {
    let peer = start(Limits::default()).await;
    let reader = Identity::generate();
    peer.trust.add(reader.public_key(), PeerKind::Guest);
    let token = token(&peer, &reader).await.unwrap();
    let s = handle(&token);
    let t = ticket(&token, "/rpm/track/", "RPM 1");
    let target = format!("/rpm/track/RPM%201?s={s}&t={t}");
    assert_eq!(get(peer.port, &target, None).await.status, 200);
    let wrong = format!("/rpm/track/RPM%202?s={s}&t={t}");
    assert_eq!(get(peer.port, &wrong, None).await.status, 401);
}

#[tokio::test]
async fn delete_is_405() {
    let peer = start(Limits::default()).await;
    let reply = send(peer.port, "DELETE /rpm/hello HTTP/1.1", b"").await;
    assert_eq!(reply.status, 405);
}

#[tokio::test]
async fn body_over_the_limit_is_413() {
    let peer = start(Limits {
        max_body: 10,
        ..Limits::default()
    })
    .await;
    let reader = Identity::generate();
    peer.trust.add(reader.public_key(), PeerKind::Account);
    let token = token(&peer, &reader).await.unwrap();
    let head = format!("POST /rpm/nas/publish HTTP/1.1\r\nAuthorization: Bearer {token}");
    assert_eq!(send(peer.port, &head, &[b'x'; 11]).await.status, 413);
    assert_eq!(send(peer.port, &head, &[b'x'; 10]).await.status, 200);
}

#[tokio::test]
async fn silent_connection_is_closed_after_request_wait() {
    let peer = start(Limits {
        request_wait: Duration::from_millis(200),
        ..Limits::default()
    })
    .await;
    let mut tcp = TcpStream::connect(("127.0.0.1", peer.port)).await.unwrap();
    let started = Instant::now();
    let mut buffer = [0u8; 16];
    let read = tokio::time::timeout(Duration::from_secs(5), tcp.read(&mut buffer))
        .await
        .expect("the server closes the connection");
    assert_eq!(read.unwrap_or(0), 0);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn connection_over_the_limit_is_closed() {
    let peer = start(Limits {
        max_connections: 2,
        request_wait: Duration::from_secs(5),
        ..Limits::default()
    })
    .await;
    let _first = TcpStream::connect(("127.0.0.1", peer.port)).await.unwrap();
    let _second = TcpStream::connect(("127.0.0.1", peer.port)).await.unwrap();
    // Give the server time to take both before the third arrives.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut third = TcpStream::connect(("127.0.0.1", peer.port)).await.unwrap();
    let mut buffer = [0u8; 16];
    let read = tokio::time::timeout(Duration::from_secs(1), third.read(&mut buffer))
        .await
        .expect("the third connection is closed at once");
    assert_eq!(read.unwrap_or(0), 0);
}
