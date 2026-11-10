//! Sessions: a challenge, a signed answer, and a token. The same rules as `PeerSessions` in `RPM.Core`.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use rpm_wire::base::{from_base64url, to_base64url};
use rpm_wire::{SessionChallenge, SessionGrant, SessionProof};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::identity::verify;
use crate::trust::{standing, Reader, Trust};

/// How long a token works.
pub const TOKEN_LIFE: Duration = Duration::from_secs(30 * 60);
/// How long a challenge can be answered.
pub const CHALLENGE_LIFE: Duration = Duration::from_secs(60);
/// The most challenges kept open. A new one beyond this drops the oldest, so a stranger asking
/// without end cannot grow the memory.
pub const MAX_CHALLENGES: usize = 256;

const PURPOSE: &str = "rpm-share-session";
const HANDLE_PURPOSE: &str = "rpm-media-handle";
const TICKET_PURPOSE: &str = "rpm-media";

/// Where the time comes from. Tests pass their own.
pub type Clock = Box<dyn Fn() -> Instant + Send + Sync>;

pub struct Sessions {
    device_id: String,
    now: Clock,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    challenges: HashMap<String, Instant>,
    issued: VecDeque<String>,
    tokens: HashMap<String, Session>,
    handles: HashMap<String, String>,
}

struct Session {
    public_key: Vec<u8>,
    until: Instant,
}

impl Sessions {
    /// Sessions for the peer with `device_id`. The id is part of what a reader signs, so an answer
    /// made for another machine is refused here.
    pub fn new(device_id: impl Into<String>) -> Sessions {
        Sessions::with_clock(device_id, Box::new(Instant::now))
    }

    pub fn with_clock(device_id: impl Into<String>, now: Clock) -> Sessions {
        Sessions {
            device_id: device_id.into(),
            now,
            inner: Mutex::new(Inner::default()),
        }
    }

    /// A new challenge.
    pub fn issue(&self) -> SessionChallenge {
        let challenge = to_base64url(&random::<24>());
        let now = (self.now)();
        let mut inner = self.inner.lock().unwrap();
        inner.sweep(now);
        while inner.challenges.len() >= MAX_CHALLENGES {
            match inner.issued.pop_front() {
                Some(oldest) => {
                    inner.challenges.remove(&oldest);
                }
                None => break,
            }
        }
        inner
            .challenges
            .insert(challenge.clone(), now + CHALLENGE_LIFE);
        inner.issued.push_back(challenge.clone());
        SessionChallenge {
            challenge,
            expires_in: CHALLENGE_LIFE.as_secs() as i32,
        }
    }

    /// A token for a correctly signed answer from a trusted key, or `None`. The challenge is used up
    /// even when the answer is refused.
    pub fn redeem(&self, proof: &SessionProof, trust: &dyn Trust) -> Option<SessionGrant> {
        let public_key = from_base64url(&proof.public_key)?;
        let signature = from_base64url(&proof.signature)?;
        let now = (self.now)();
        {
            let mut inner = self.inner.lock().unwrap();
            inner.sweep(now);
            match inner.challenges.remove(&proof.challenge) {
                Some(expires) if expires > now => {}
                _ => return None,
            }
        }
        let reader = standing(trust, &public_key)?;
        let signed = signed_bytes(&self.device_id, &proof.challenge, &proof.account);
        if !verify(&public_key, &signed, &signature) {
            return None;
        }
        let token = to_base64url(&random::<32>());
        let mut inner = self.inner.lock().unwrap();
        inner.handles.insert(handle(&token), token.clone());
        inner.tokens.insert(
            token.clone(),
            Session {
                public_key,
                until: now + TOKEN_LIFE,
            },
        );
        Some(SessionGrant {
            token,
            expires_in: TOKEN_LIFE.as_secs() as i32,
            kind: reader.kind,
        })
    }

    /// The reader behind a token, checked against the trust list as it is now.
    pub fn reader(&self, token: &str, trust: &dyn Trust) -> Option<Reader> {
        if token.is_empty() {
            return None;
        }
        let public_key = {
            let mut inner = self.inner.lock().unwrap();
            inner.sweep((self.now)());
            inner.tokens.get(token)?.public_key.clone()
        };
        standing(trust, &public_key)
    }

    /// The reader behind a media URL: the handle names the token, and the ticket must be the one
    /// that token makes for this route and id.
    pub fn reader_for_ticket(
        &self,
        handle: &str,
        ticket_text: &str,
        route: &str,
        id: &str,
        trust: &dyn Trust,
    ) -> Option<Reader> {
        if handle.is_empty() || ticket_text.is_empty() {
            return None;
        }
        let token = {
            let mut inner = self.inner.lock().unwrap();
            inner.sweep((self.now)());
            inner.handles.get(handle)?.clone()
        };
        let expected = ticket(&token, route, id);
        if expected.as_bytes().ct_eq(ticket_text.as_bytes()).into() {
            self.reader(&token, trust)
        } else {
            None
        }
    }
}

impl Inner {
    fn sweep(&mut self, now: Instant) {
        self.challenges.retain(|_, expires| *expires > now);
        while let Some(first) = self.issued.front() {
            if self.challenges.contains_key(first) {
                break;
            }
            self.issued.pop_front();
        }
        let expired: Vec<String> = self
            .tokens
            .iter()
            .filter(|(_, session)| session.until <= now)
            .map(|(token, _)| token.clone())
            .collect();
        for token in expired {
            self.tokens.remove(&token);
            self.handles.remove(&handle(&token));
        }
    }
}

/// What a reader signs: `rpm-share-session`, this peer's device id, the challenge and the reader's
/// account, one per line.
pub fn signed_bytes(server_device_id: &str, challenge: &str, account: &str) -> Vec<u8> {
    format!("{PURPOSE}\n{server_device_id}\n{challenge}\n{account}").into_bytes()
}

/// The handle a media URL carries in `s`: it names the token without giving it away.
pub fn handle(token: &str) -> String {
    let hash = Sha256::digest(format!("{HANDLE_PURPOSE}\n{token}").as_bytes());
    to_base64url(&hash[..16])
}

/// The ticket a media URL carries in `t`: it opens one route and id only.
pub fn ticket(token: &str, route: &str, id: &str) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(token.as_bytes()).expect("HMAC takes a key of any length");
    mac.update(format!("{TICKET_PURPOSE}\n{route}{id}").as_bytes());
    to_base64url(&mac.finalize().into_bytes()[..16])
}

fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    OsRng.fill_bytes(&mut bytes);
    bytes
}
