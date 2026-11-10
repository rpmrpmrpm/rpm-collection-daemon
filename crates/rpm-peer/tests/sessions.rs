mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::TestTrust;
use rpm_peer::identity::Identity;
use rpm_peer::sessions::{handle, signed_bytes, ticket, Sessions, MAX_CHALLENGES};
use rpm_wire::base::to_base64url;
use rpm_wire::{PeerKind, SessionProof};
use serde_json::Value;

const SERVER: &str = "SERVER";

fn clocked() -> (Sessions, Arc<Mutex<Instant>>) {
    let now = Arc::new(Mutex::new(Instant::now()));
    let read = now.clone();
    let sessions = Sessions::with_clock(SERVER, Box::new(move || *read.lock().unwrap()));
    (sessions, now)
}

fn proof(identity: &Identity, challenge: &str, server: &str) -> SessionProof {
    let signature = identity.sign(&signed_bytes(server, challenge, "user-1"));
    SessionProof {
        public_key: to_base64url(identity.public_key()),
        challenge: challenge.to_string(),
        signature: to_base64url(&signature),
        account: "user-1".to_string(),
    }
}

#[test]
fn trusted_key_gets_a_token_of_its_kind() {
    let (sessions, _) = clocked();
    let trust = TestTrust::default();
    let reader = Identity::generate();
    trust.add(reader.public_key(), PeerKind::Guest);
    let challenge = sessions.issue().challenge;
    let grant = sessions
        .redeem(&proof(&reader, &challenge, SERVER), &trust)
        .unwrap();
    assert_eq!(grant.kind, PeerKind::Guest);
    assert_eq!(grant.expires_in, 1800);
    assert!(sessions.reader(&grant.token, &trust).is_some());
}

#[test]
fn challenge_is_single_use_and_expires_after_60_s() {
    let (sessions, now) = clocked();
    let trust = TestTrust::default();
    let reader = Identity::generate();
    trust.add(reader.public_key(), PeerKind::Account);

    let used = sessions.issue().challenge;
    assert!(sessions
        .redeem(&proof(&reader, &used, SERVER), &trust)
        .is_some());
    assert!(sessions
        .redeem(&proof(&reader, &used, SERVER), &trust)
        .is_none());

    let late = sessions.issue().challenge;
    *now.lock().unwrap() += Duration::from_secs(61);
    assert!(sessions
        .redeem(&proof(&reader, &late, SERVER), &trust)
        .is_none());
}

#[test]
fn at_most_256_open_challenges_oldest_dropped() {
    let (sessions, _) = clocked();
    let trust = TestTrust::default();
    let reader = Identity::generate();
    trust.add(reader.public_key(), PeerKind::Account);
    let oldest = sessions.issue().challenge;
    for _ in 0..MAX_CHALLENGES {
        sessions.issue();
    }
    assert!(sessions
        .redeem(&proof(&reader, &oldest, SERVER), &trust)
        .is_none());
}

#[test]
fn answer_made_for_another_machine_is_refused() {
    let (sessions, _) = clocked();
    let trust = TestTrust::default();
    let reader = Identity::generate();
    trust.add(reader.public_key(), PeerKind::Account);
    let challenge = sessions.issue().challenge;
    assert!(sessions
        .redeem(&proof(&reader, &challenge, "OTHER"), &trust)
        .is_none());
}

#[test]
fn untrusted_key_gets_no_token() {
    let (sessions, _) = clocked();
    let trust = TestTrust::default();
    let stranger = Identity::generate();
    let challenge = sessions.issue().challenge;
    assert!(sessions
        .redeem(&proof(&stranger, &challenge, SERVER), &trust)
        .is_none());
}

#[test]
fn owner_key_gets_a_token_as_account_without_being_in_the_list() {
    let (sessions, _) = clocked();
    let trust = TestTrust::default();
    let owner = Identity::generate();
    trust
        .owners
        .lock()
        .unwrap()
        .insert(owner.public_key().to_vec());
    let challenge = sessions.issue().challenge;
    let grant = sessions
        .redeem(&proof(&owner, &challenge, SERVER), &trust)
        .unwrap();
    assert_eq!(grant.kind, PeerKind::Account);
    assert!(sessions.reader(&grant.token, &trust).unwrap().owner);
}

#[test]
fn token_stops_working_when_key_leaves_trust_list() {
    let (sessions, _) = clocked();
    let trust = TestTrust::default();
    let reader = Identity::generate();
    trust.add(reader.public_key(), PeerKind::Guest);
    let challenge = sessions.issue().challenge;
    let token = sessions
        .redeem(&proof(&reader, &challenge, SERVER), &trust)
        .unwrap()
        .token;
    trust.remove(reader.public_key());
    assert!(sessions.reader(&token, &trust).is_none());
}

#[test]
fn token_expires_after_30_minutes() {
    let (sessions, now) = clocked();
    let trust = TestTrust::default();
    let reader = Identity::generate();
    trust.add(reader.public_key(), PeerKind::Guest);
    let challenge = sessions.issue().challenge;
    let token = sessions
        .redeem(&proof(&reader, &challenge, SERVER), &trust)
        .unwrap()
        .token;
    *now.lock().unwrap() += Duration::from_secs(30 * 60);
    assert!(sessions.reader(&token, &trust).is_none());
}

#[test]
fn ticket_opens_only_its_route_and_id() {
    let (sessions, _) = clocked();
    let trust = TestTrust::default();
    let reader = Identity::generate();
    trust.add(reader.public_key(), PeerKind::Guest);
    let challenge = sessions.issue().challenge;
    let token = sessions
        .redeem(&proof(&reader, &challenge, SERVER), &trust)
        .unwrap()
        .token;
    let h = handle(&token);
    let t = ticket(&token, "/rpm/track/", "RPM0001");
    assert!(sessions
        .reader_for_ticket(&h, &t, "/rpm/track/", "RPM0001", &trust)
        .is_some());
    assert!(sessions
        .reader_for_ticket(&h, &t, "/rpm/track/", "RPM0002", &trust)
        .is_none());
    assert!(sessions
        .reader_for_ticket(&h, &t, "/rpm/cover/", "RPM0001", &trust)
        .is_none());
}

#[test]
fn handle_and_ticket_match_dotnet_vectors() {
    let v: Value = serde_json::from_str(include_str!("vectors/identity.json")).unwrap();
    let token = v["token"].as_str().unwrap();
    assert_eq!(handle(token), v["handle"].as_str().unwrap());
    assert_eq!(
        ticket(
            token,
            v["ticketRoute"].as_str().unwrap(),
            v["ticketId"].as_str().unwrap()
        ),
        v["ticket"].as_str().unwrap()
    );
    let data = signed_bytes(
        v["serverDeviceId"].as_str().unwrap(),
        v["challenge"].as_str().unwrap(),
        v["account"].as_str().unwrap(),
    );
    assert_eq!(to_base64url(&data), v["data"].as_str().unwrap());
}
