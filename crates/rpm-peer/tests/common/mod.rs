//! A small TLS client and a trust list for the tests.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use rpm_peer::trust::Trust;
use rpm_wire::PeerKind;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

#[derive(Default)]
pub struct TestTrust {
    pub readers: Mutex<HashMap<Vec<u8>, PeerKind>>,
    pub owners: Mutex<HashSet<Vec<u8>>>,
}

impl TestTrust {
    pub fn add(&self, key: &[u8], kind: PeerKind) {
        self.readers.lock().unwrap().insert(key.to_vec(), kind);
    }
    pub fn remove(&self, key: &[u8]) {
        self.readers.lock().unwrap().remove(key);
    }
}

impl Trust for TestTrust {
    fn kind_of(&self, public_key: &[u8]) -> Option<PeerKind> {
        self.readers.lock().unwrap().get(public_key).copied()
    }
    fn is_owner(&self, public_key: &[u8]) -> bool {
        self.owners.lock().unwrap().contains(public_key)
    }
}

/// Accepts any certificate. The tests check the server, not the pinning, which the clients do.
#[derive(Debug)]
struct AnyCertificate;

impl ServerCertVerifier for AnyCertificate {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

pub struct Reply {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

/// Sends `head` (request line and headers, without the blank line) and `body`, and reads the whole
/// reply. The server closes the connection after one request.
pub async fn send(port: u16, head: &str, body: &[u8]) -> Reply {
    let config =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AnyCertificate))
            .with_no_client_auth();
    let tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut tls = TlsConnector::from(Arc::new(config))
        .connect(ServerName::try_from("localhost").unwrap(), tcp)
        .await
        .unwrap();
    let mut request = format!(
        "{head}\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    request.extend_from_slice(body);
    tls.write_all(&request).await.unwrap();
    let mut raw = Vec::new();
    let _ = tls.read_to_end(&mut raw).await;
    parse(&raw)
}

pub async fn get(port: u16, target: &str, token: Option<&str>) -> Reply {
    let head = match token {
        Some(token) => format!("GET {target} HTTP/1.1\r\nAuthorization: Bearer {token}"),
        None => format!("GET {target} HTTP/1.1"),
    };
    send(port, &head, b"").await
}

fn parse(raw: &[u8]) -> Reply {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("a reply with headers");
    let head = std::str::from_utf8(&raw[..split]).unwrap();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect();
    Reply {
        status,
        headers,
        body: raw[split + 4..].to_vec(),
    }
}
