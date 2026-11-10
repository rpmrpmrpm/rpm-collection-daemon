# Contributing to the rpm collection daemon

Thank you for helping. This repo holds the rpm daemon for NAS systems. The rpm apps are in the other
rpm repos; see [CONTRIBUTING.md in `rpm`](https://github.com/rpmrpmrpm/rpm/blob/main/CONTRIBUTING.md)
for the list.

## Where things go

| You want to | Go to |
| --- | --- |
| Suggest an idea or ask a question | [Discussions](https://github.com/rpmrpmrpm/rpm/discussions) in the `rpm` repo |
| Report a bug in the daemon | Issues in this repo |
| Report a security problem | Do not open an issue. See [SECURITY.md](SECURITY.md). |
| Add a package for another NAS system | A pull request here. See [docs/PORTING.md](docs/PORTING.md). |

## Before you start

- For a small fix, open a pull request directly.
- For a new feature or a larger change, open a discussion or an issue first, so we can agree on the
  approach before you spend time on it.
- A change to the protocol needs the same change in the rpm apps. Open an issue first.

## Pull requests

- Keep a pull request to one change. Small pull requests are reviewed faster.
- Fill in the pull request template: what changes, why, and how you tested it.
- Add or update tests.
- Every pull request gets a review from a maintainer. An AI reviewer may also comment. A maintainer
  makes the final decision.

### Sign your commits (DCO)

rpm uses the [Developer Certificate of Origin](https://developercertificate.org/) instead of a
contributor licence agreement. By adding a `Signed-off-by` line to a commit, you state that you wrote
the change or have the right to submit it under the project's licence.

```
git commit -s -m "collection: answer a missing cover with 404"
```

A check on every pull request fails if a commit is not signed off. To fix older commits on your
branch:

```
git rebase --signoff main
git push --force-with-lease
```

## Building and testing

You need [Rust](https://rustup.rs) (stable). Before you open a pull request:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Writing for rpm

### Code comments

Comments are read by people who have not seen the history of the code, and many of them do not have
English as a first language.

- Say what the code does and why, as it is now. Keep warnings about traps.
- No history: no dates, names or "used to". That belongs in the commit message.
- Plain international English: short sentences, common words, no idioms. Use British spelling
  (colour, centre, catalogue, licence, behaviour).
- Keep identifiers as they are, and use the code's own names for things.

### Commit messages

Use the form `area: what changes`, for example `peer: close a connection that sends nothing`.
Explain in the body why the change is needed if that is not obvious.

## Licence

The code is under the [Mozilla Public License 2.0](LICENSE). Your contribution is under the same
licence. The name "rpm" and the logo are not covered by the code licence. See
[TRADEMARKS.md](TRADEMARKS.md).

## Money

rpm has paid supporters. Their money pays for the servers, storage and development. Contributions
are voluntary and unpaid, and contributing does not give a share of that money.
