# Packaging the daemon for another system

The daemon is one static binary with no dependencies. A package for a new NAS system has three jobs:

1. Put the binary on the system.
2. Write the configuration file from the system's own install screens.
3. Start the binary at boot and stop it at shutdown.

## The configuration

| Setting | Meaning |
| --- | --- |
| `root` | The folder that holds the libraries. rpm makes one subfolder per person. The daemon only reads it. |
| `limit` | The space limit for all libraries together, for example `2TB`. Leave it out for no limit. |
| `name` | The name rpm shows. Default: the host name. |
| `data` | A folder the daemon may write, for its keys and the uploaded catalogues. Never inside `root`. |
| `smb_share` | How a computer reaches `root` over SMB, for example `smb://nas.local/music/rpm`. rpm mounts this address. |
| `port` | Optional. The first of a range of fixed ports, for a firewall. |

Start the daemon with `rpm-collection-daemon --config <file>`. Every setting can also be given as an
environment variable in capitals with an `RPM_` prefix (`RPM_ROOT`, `RPM_LIMIT`, ...), which wins over
the file. The daemon logs to standard error; `RPM_LOG=debug` gives more.

What the system must allow:

- **Multicast DNS** on UDP port 5353, so rpm can find the NAS. In Docker this needs host networking.
- **TCP connections** to the daemon's ports from the local network. By default the system chooses
  the ports. Set `port` when a firewall needs fixed ones; the NAS takes `port` and the libraries the
  next ports.
- **Read access** to `root` and everything in it, and write access to `data`.
- **SMB** for `root`, for the people who use rpm. The daemon does not need it, but rpm on their
  computers writes the music through it.

## Building

With [Rust](https://rustup.rs) and [cargo-zigbuild](https://github.com/rust-cross/cargo-zigbuild):

```sh
cargo zigbuild --release -p rpm-daemon --target x86_64-unknown-linux-musl
cargo zigbuild --release -p rpm-daemon --target aarch64-unknown-linux-musl
```

The binary is `target/<target>/release/rpm-collection-daemon`. For another architecture, add its
musl target with `rustup target add` and build the same way. For a system with its own C library and
no musl, `cargo build --release` on the system itself works too.

## Checking it

On another machine on the same network:

```sh
dns-sd -B _rpm-nas._tcp           # macOS
avahi-browse -r _rpm-nas._tcp     # Linux
```

You should see the NAS by its name, and one entry for each library folder that has a
`.rpm/library.json`. Then pick the NAS in rpm's setup.

## Examples in this repo

- `packaging/linux`: a systemd service and an install script.
- `packaging/docker`: the image, a Compose file, and listings for TrueNAS, Unraid and CasaOS.
- `packaging/synology`: a DSM 7 package with an install wizard.
