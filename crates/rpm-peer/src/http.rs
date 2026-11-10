//! The HTTPS server. It owns the socket, the limits, the greeting, the session routes, and the rule
//! that nothing else is answered without a token. What a route answers is decided by [`Routes`].
//!
//! Each connection carries one request and is then closed, as `LibraryServer` in `RPM.Core` does.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Empty, Full, LengthLimitError, Limited};
use hyper::body::Incoming;
use hyper::header::{HeaderMap, HeaderValue, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use rpm_wire::{routes, PeerHello, SessionProof};
use rustls::ServerConfig;
use serde::Serialize;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;

use crate::sessions::Sessions;
use crate::trust::{Reader, Trust};

/// A response body: empty, bytes in memory, or a file streamed from the disk.
pub type Body = BoxBody<Bytes, std::io::Error>;

/// The most a request line and its headers may take.
const MAX_HEADER_BYTES: usize = 16 * 1024;
/// The most a session answer may take.
const MAX_PROOF_BYTES: usize = 8 * 1024;

/// A request from a reader that has proved its key.
pub struct AuthedRequest {
    pub method: Method,
    /// The path as it arrived, still percent-encoded. Use [`tail`] to read an id from it.
    pub path: String,
    /// The query, decoded.
    pub query: HashMap<String, String>,
    pub headers: HeaderMap,
    /// The body of a POST or PUT, read whole within [`Limits::max_body`]. Empty otherwise.
    pub body: Bytes,
    pub reader: Reader,
}

/// What the peer serves.
#[async_trait]
pub trait Routes: Send + Sync + 'static {
    /// The greeting, which anyone may read.
    fn hello(&self) -> PeerHello;

    /// Every route except the greeting and the session routes.
    async fn handle(&self, request: AuthedRequest) -> Response<Body>;
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// How long a connection gets for the TLS handshake and for its request headers. A connection that
    /// sends nothing is closed after this, so silent connections cannot use up the sockets.
    pub request_wait: Duration,
    /// Connections served at once. One over the limit is closed at once.
    pub max_connections: usize,
    /// The largest POST or PUT body. A larger one is answered 413.
    pub max_body: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            request_wait: Duration::from_secs(15),
            max_connections: 64,
            max_body: 64 * 1024 * 1024,
        }
    }
}

struct State {
    sessions: Arc<Sessions>,
    trust: Arc<dyn Trust>,
    routes: Arc<dyn Routes>,
    limits: Limits,
}

/// Serves `listener` until `stop` is cancelled.
pub async fn serve(
    listener: TcpListener,
    tls: Arc<ServerConfig>,
    sessions: Arc<Sessions>,
    trust: Arc<dyn Trust>,
    routes: Arc<dyn Routes>,
    limits: Limits,
    stop: CancellationToken,
) {
    let acceptor = TlsAcceptor::from(tls);
    let connections = Arc::new(Semaphore::new(limits.max_connections));
    let state = Arc::new(State {
        sessions,
        trust,
        routes,
        limits,
    });
    loop {
        let tcp = tokio::select! {
            _ = stop.cancelled() => return,
            accepted = listener.accept() => match accepted {
                Ok((tcp, _)) => tcp,
                Err(_) => {
                    // Out of file descriptors, or a connection reset before it was accepted. Wait a
                    // little so the loop does not spin.
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
            },
        };
        let Ok(permit) = connections.clone().try_acquire_owned() else {
            drop(tcp);
            continue;
        };
        let acceptor = acceptor.clone();
        let state = state.clone();
        let stop = stop.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let wait = state.limits.request_wait;
            let Ok(Ok(stream)) = tokio::time::timeout(wait, acceptor.accept(tcp)).await else {
                return;
            };
            let service = service_fn(move |request| {
                let state = state.clone();
                async move { Ok::<_, Infallible>(answer(request, state).await) }
            });
            let mut builder = http1::Builder::new();
            builder
                .timer(TokioTimer::new())
                .header_read_timeout(wait)
                .keep_alive(false)
                .max_buf_size(MAX_HEADER_BYTES);
            let connection = builder.serve_connection(TokioIo::new(stream), service);
            tokio::select! {
                _ = connection => {}
                _ = stop.cancelled() => {}
            }
        });
    }
}

async fn answer(request: Request<Incoming>, state: Arc<State>) -> Response<Body> {
    let method = request.method().clone();
    if !matches!(
        method,
        Method::GET | Method::HEAD | Method::POST | Method::PUT
    ) {
        return status(StatusCode::METHOD_NOT_ALLOWED);
    }
    let head_only = method == Method::HEAD;
    let path = request.uri().path().to_string();
    let query = parse_query(request.uri().query().unwrap_or(""));

    // The greeting is the one thing anyone may read.
    if path == routes::HELLO {
        return json(&state.routes.hello(), head_only);
    }

    if path == routes::SESSION {
        if method != Method::POST {
            return json(&state.sessions.issue(), head_only);
        }
        let Ok(body) = read_body(request.into_body(), MAX_PROOF_BYTES).await else {
            return status(StatusCode::FORBIDDEN);
        };
        let grant = serde_json::from_slice::<SessionProof>(&body)
            .ok()
            .and_then(|proof| state.sessions.redeem(&proof, &*state.trust));
        return match grant {
            Some(grant) => json(&grant, false),
            None => status(StatusCode::FORBIDDEN),
        };
    }

    // A token in the header opens everything. A media URL carries a ticket for its own route and id
    // instead, so a URL read off the network opens that one track or picture only.
    let reader = match bearer(request.headers()) {
        Some(token) => state.sessions.reader(&token, &*state.trust),
        None => media_target(&path).and_then(|(route, id)| {
            state.sessions.reader_for_ticket(
                query.get("s").map(String::as_str).unwrap_or(""),
                query.get("t").map(String::as_str).unwrap_or(""),
                route,
                &id,
                &*state.trust,
            )
        }),
    };
    let Some(reader) = reader else {
        return status(StatusCode::UNAUTHORIZED);
    };

    let headers = request.headers().clone();
    let body = if matches!(method, Method::POST | Method::PUT) {
        match read_body(request.into_body(), state.limits.max_body).await {
            Ok(body) => body,
            Err(BodyError::TooLarge) => return status(StatusCode::PAYLOAD_TOO_LARGE),
            Err(BodyError::Broken) => return status(StatusCode::BAD_REQUEST),
        }
    } else {
        Bytes::new()
    };

    state
        .routes
        .handle(AuthedRequest {
            method,
            path,
            query,
            headers,
            body,
            reader,
        })
        .await
}

enum BodyError {
    TooLarge,
    /// The body ended early or could not be read: the reader hung up during an upload.
    Broken,
}

async fn read_body(body: Incoming, limit: usize) -> Result<Bytes, BodyError> {
    match Limited::new(body, limit).collect().await {
        Ok(collected) => Ok(collected.to_bytes()),
        Err(error) if error.downcast_ref::<LengthLimitError>().is_some() => {
            Err(BodyError::TooLarge)
        }
        Err(_) => Err(BodyError::Broken),
    }
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(hyper::header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_at_checked(7)?;
    if scheme.eq_ignore_ascii_case("Bearer ") {
        let token = token.trim();
        (!token.is_empty()).then(|| token.to_string())
    } else {
        None
    }
}

fn media_target(path: &str) -> Option<(&'static str, String)> {
    routes::MEDIA
        .iter()
        .find_map(|route| tail(path, route).map(|id| (*route, id)))
}

/// The decoded rest of `path` after `prefix`, or `None` when the path does not start with it or
/// nothing follows.
pub fn tail(path: &str, prefix: &str) -> Option<String> {
    let rest = path.strip_prefix(prefix)?;
    let decoded = percent_decode(rest);
    (!decoded.is_empty()).then_some(decoded)
}

fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((name, value)) => (percent_decode(name), percent_decode(value)),
            None => (percent_decode(pair), String::new()),
        })
        .collect()
}

/// Decodes `%XX` sequences. A `+` stays a `+`, as with .NET's `Uri.UnescapeDataString`.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(high << 4 | low);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// An empty body.
pub fn empty() -> Body {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed()
}

/// A body held in memory.
pub fn full(bytes: impl Into<Bytes>) -> Body {
    Full::new(bytes.into())
        .map_err(|never| match never {})
        .boxed()
}

/// A response with no body.
pub fn status(code: StatusCode) -> Response<Body> {
    let mut response = Response::new(empty());
    *response.status_mut() = code;
    response
        .headers_mut()
        .insert(CONTENT_LENGTH, HeaderValue::from_static("0"));
    response
}

/// A JSON response. For HEAD, only the headers are sent.
pub fn json<T: Serialize>(value: &T, head_only: bool) -> Response<Body> {
    match serde_json::to_vec(value) {
        Ok(bytes) => bytes_response(
            StatusCode::OK,
            "application/json; charset=utf-8",
            Bytes::from(bytes),
            head_only,
            None,
        ),
        Err(_) => status(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// A response with `bytes` as its body, its type and length, and an optional cache rule. For HEAD,
/// only the headers are sent.
pub fn bytes_response(
    code: StatusCode,
    content_type: &str,
    bytes: Bytes,
    head_only: bool,
    cache: Option<&'static str>,
) -> Response<Body> {
    let length = bytes.len();
    let mut response = Response::new(if head_only { empty() } else { full(bytes) });
    *response.status_mut() = code;
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(content_type) {
        headers.insert(CONTENT_TYPE, value);
    }
    headers.insert(CONTENT_LENGTH, HeaderValue::from(length));
    if let Some(cache) = cache {
        headers.insert(CACHE_CONTROL, HeaderValue::from_static(cache));
    }
    response
}
