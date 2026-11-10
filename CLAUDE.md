# rpm-collection-daemon

Notes for AI coding assistants (and people) working in this repo. Read CONTRIBUTING.md first; its
rules on comments and commit sign-off apply to every change.

## This repo

The rpm daemon for NAS systems, in Rust. It serves library folders over the rpm peer protocol that
`rpm-desktop/RPM.Core/Sharing` defines.

- Test: `cargo test --workspace`. Lint: `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo fmt --check`. Add no new warnings.
- The wire must match `RPM.Core/Sharing` exactly. `docs/PROTOCOL.md` describes it, and
  `crates/rpm-peer/tests/vectors/identity.json` is a copy of the vector file in `rpm-desktop`. Keep
  both copies equal.
- Keep the binary small and static. Ask before adding a dependency that pulls in a runtime, a
  database or OpenSSL.

## Comments

Comments are read by contributors who have not seen the history of the code, and many of them do
not have English as a first language. Write for them.

- Say what the code does and why, as it is now. Keep warnings about traps.
- No history: no dates, no names, no quotes from a conversation, no "used to", "was", "until", and
  no notes about code that has been removed. That belongs in the commit message.
- Plain international English: short sentences, common words, no idioms. Use British spelling
  (colour, centre, catalogue, licence, behaviour). No em-dashes.
- Keep identifiers as they are. Use the code's own names for things.

## Do not

- Do not add a web interface.
- Do not add secrets, API keys or personal data to the repo.
