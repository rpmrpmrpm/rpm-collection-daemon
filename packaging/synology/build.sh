#!/bin/sh
# Builds a DSM 7 package from a static binary.
#
#   packaging/synology/build.sh <x86_64|armv8> <path to rpm-collection-daemon> <version> <output folder>
#
# x86_64 covers every 64-bit Intel and AMD model, armv8 every 64-bit ARM model.
set -eu

ARCH=$1
BINARY=$2
VERSION=$3
OUT=$4
HERE=$(cd "$(dirname "$0")" && pwd)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$WORK/package/bin" "$OUT"
install -m 0755 "$BINARY" "$WORK/package/bin/rpm-collection-daemon"
tar -czf "$WORK/package.tgz" -C "$WORK/package" .

cat > "$WORK/INFO" <<INFO
package="rpm-collection-daemon"
version="$VERSION-1"
os_min_ver="7.0-40000"
arch="$ARCH"
displayname="rpm collection"
description="Keeps rpm music libraries on this NAS. rpm on your computer finds the NAS and offers it as a place for your collection. Your TVs and the people you share with play it from here, also when your computer is off."
maintainer="rpm"
maintainer_url="https://github.com/rpmrpmrpm/rpm-collection-daemon"
support_url="https://github.com/rpmrpmrpm/rpm/discussions"
thirdparty="yes"
startable="yes"
ctl_stop="yes"
checksum="$(md5sum "$WORK/package.tgz" 2>/dev/null | cut -d' ' -f1 || md5 -q "$WORK/package.tgz")"
INFO

cp -R "$HERE/conf" "$HERE/scripts" "$HERE/WIZARD_UIFILES" "$WORK/"
cp "$HERE/PACKAGE_ICON.PNG" "$HERE/PACKAGE_ICON_256.PNG" "$WORK/"
(cd "$WORK" && tar -cf "$OUT/rpm-collection-daemon-$ARCH-$VERSION.spk" \
    INFO package.tgz conf scripts WIZARD_UIFILES PACKAGE_ICON.PNG PACKAGE_ICON_256.PNG)
echo "$OUT/rpm-collection-daemon-$ARCH-$VERSION.spk"
