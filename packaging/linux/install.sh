#!/bin/sh
# Installs the rpm collection daemon as a systemd service on Linux (x86-64 or ARM64).
#
#   sudo ./install.sh                 asks for each setting
#   sudo RPM_ROOT=/srv/music/rpm RPM_SMB_SHARE=smb://nas.local/music/rpm RPM_LIMIT=2TB ./install.sh
#
# The binary is taken from next to this script when it is there (rpm-collection-daemon), and
# downloaded from the latest GitHub release otherwise.
set -eu

REPO="rpmrpmrpm/rpm-collection-daemon"
BIN=/usr/local/bin/rpm-collection-daemon
CONF_DIR=/etc/rpm-collection-daemon
UNIT=/etc/systemd/system/rpm-collection-daemon.service
USER_NAME=rpm-collection
HERE=$(cd "$(dirname "$0")" && pwd)

if [ "$(id -u)" -ne 0 ]; then
    echo "Run this as root (sudo)." >&2
    exit 1
fi

case "$(uname -m)" in
    x86_64|amd64) TARGET=x86_64-unknown-linux-musl ;;
    aarch64|arm64) TARGET=aarch64-unknown-linux-musl ;;
    *) echo "This script supports x86-64 and ARM64. Build the daemon yourself for $(uname -m)." >&2; exit 1 ;;
esac

ask() {
    # ask <variable> <question> <default>
    eval "current=\${$1:-}"
    if [ -z "$current" ] && [ -t 0 ]; then
        printf '%s [%s]: ' "$2" "$3"
        read -r current || true
    fi
    [ -n "$current" ] || current=$3
    eval "$1=\$current"
}

HOST=$(hostname -s 2>/dev/null || hostname)
ask RPM_ROOT "Folder that holds the libraries" "/srv/music/rpm"
ask RPM_SMB_SHARE "How computers reach that folder over SMB" "smb://$HOST.local/music/rpm"
ask RPM_LIMIT "Space limit, for example 2TB (empty for none)" ""
ask RPM_NAME "Name rpm shows for this machine" "$HOST"

mkdir -p "$RPM_ROOT"

if [ -x "$HERE/rpm-collection-daemon" ]; then
    install -m 0755 "$HERE/rpm-collection-daemon" "$BIN"
else
    URL="https://github.com/$REPO/releases/latest/download/rpm-collection-daemon-$TARGET"
    echo "Downloading $URL"
    curl -fsSL "$URL" -o "$BIN.new"
    chmod 0755 "$BIN.new"
    mv "$BIN.new" "$BIN"
fi

if ! id "$USER_NAME" >/dev/null 2>&1; then
    useradd --system --no-create-home --shell /usr/sbin/nologin "$USER_NAME"
fi

mkdir -p "$CONF_DIR"
{
    echo "root = \"$RPM_ROOT\""
    echo "data = \"/var/lib/rpm-collection-daemon\""
    echo "smb_share = \"$RPM_SMB_SHARE\""
    echo "name = \"$RPM_NAME\""
    [ -n "$RPM_LIMIT" ] && echo "limit = \"$RPM_LIMIT\""
} > "$CONF_DIR/config.toml"
chmod 0644 "$CONF_DIR/config.toml"

if [ -f "$HERE/rpm-collection-daemon.service" ]; then
    install -m 0644 "$HERE/rpm-collection-daemon.service" "$UNIT"
else
    curl -fsSL "https://raw.githubusercontent.com/$REPO/main/packaging/linux/rpm-collection-daemon.service" -o "$UNIT"
fi

# The daemon must be able to read the music folder: give it the folder's group.
GROUP=$(stat -c %G "$RPM_ROOT")
mkdir -p "$UNIT.d"
printf '[Service]\nSupplementaryGroups=%s\nReadOnlyPaths=%s\n' "$GROUP" "$RPM_ROOT" > "$UNIT.d/music.conf"

systemctl daemon-reload
systemctl enable --now rpm-collection-daemon
echo "Installed. Status: systemctl status rpm-collection-daemon"
echo "The libraries folder must be readable by the group $GROUP and shared over SMB as $RPM_SMB_SHARE."
