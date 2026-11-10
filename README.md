# rpm collection daemon

A small service for a NAS that makes a folder on the NAS an rpm library. rpm on your computer finds
the NAS on the local network and offers it as a place to keep your collection. Your TVs, and the
people you share your collection with, play it from the NAS, also when your computer is off.

Part of [rpm](https://github.com/rpmrpmrpm/rpm). Ideas and questions go to the
[discussions](https://github.com/rpmrpmrpm/rpm/discussions) there.

## What it does

- It announces the NAS, and each library folder on it, on the local network (multicast DNS,
  service `_rpm-nas._tcp`).
- It serves each library to the rpm apps over HTTPS, with the same protocol and the same trust rules
  as an rpm computer that shares its collection.
- It does not read tags, analyse music or keep its own index. The owner's computer writes the files
  over SMB and sends the daemon the catalogue it already keeps.
- It has no web interface. It reads one configuration file.

## Install

| System | How |
| --- | --- |
| Synology DSM 7 | The `.spk` package from the releases page, installed by hand in Package Center |
| Docker (TrueNAS SCALE, Unraid, QNAP Container Station, UGREEN, CasaOS, any Linux) | `packaging/docker/compose.yaml`. It needs host networking. |
| Linux (x86-64 or ARM64) | `packaging/linux/install.sh`, which installs the binary and a systemd service |

To package it for another system, see [docs/PORTING.md](docs/PORTING.md).

## Configuration

| Setting | Environment variable | Meaning |
| --- | --- | --- |
| `root` | `RPM_ROOT` | The folder that holds the libraries. Each subfolder is one person's library. |
| `limit` | `RPM_LIMIT` | The space limit for all libraries together, for example `2TB`. Empty for no limit. |
| `name` | `RPM_NAME` | The NAS name to announce. Default: the host name. |
| `data` | `RPM_DATA` | Where the daemon keeps its keys and the uploaded catalogues. Never inside `root`. |
| `smb_share` | `RPM_SMB_SHARE` | How computers reach `root` over SMB, for example `smb://nas.local/music/rpm`. |

See `config.example.toml`.

## Build

You need [Rust](https://rustup.rs) (stable).

```sh
cargo test --workspace
cargo build --release -p rpm-daemon
```

Static Linux binaries, for any NAS, with [cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild):

```sh
cargo zigbuild --release -p rpm-daemon --target x86_64-unknown-linux-musl
cargo zigbuild --release -p rpm-daemon --target aarch64-unknown-linux-musl
```

## What is here

| Folder | What it is |
| --- | --- |
| `crates/rpm-wire` | The protocol types. No input or output. |
| `crates/rpm-peer` | The device key, certificate, sessions, trust check, HTTPS server and announcements. |
| `crates/rpm-collection` | One library folder: uploaded catalogues, tracks, covers, space used. |
| `crates/rpm-daemon` | The program: reads the configuration and runs everything. |
| `packaging/` | The Linux, Docker and Synology packages. |
| `docs/` | The protocol, how to package for a new system, and the concurrency notes. |

## Licence

[Mozilla Public License 2.0](LICENSE). The rpm name and logo are not covered by the code licence; see
[TRADEMARKS.md](TRADEMARKS.md).
