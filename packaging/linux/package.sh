#!/usr/bin/env bash
# Build and package Rewind OS for Linux as a portable tar.gz:
#
#   rewind-os-<version>-linux-<arch>.tar.gz
#     rewind-os-<version>-linux-<arch>/
#       bin/rewind-os, bin/rewind-os-cli
#       share/applications/io.github.r_dvl.rewind_os.desktop
#       share/icons/hicolor/<n>x<n>/apps/io.github.r_dvl.rewind_os.png
#       README.md, README.es.md, LICENSE-MIT, LICENSE-APACHE, NOTICE
#
# Unpack it anywhere and run bin/rewind-os; copy share/ into ~/.local/share to get the
# launcher entry and icon.
#
#   packaging/linux/package.sh [--skip-build]
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

VERSION="${REWIND_OS_VERSION:-$(awk '/^\[workspace.package\]/{p=1;next} /^\[/{p=0} p && /^version *=/{gsub(/.*= *"|".*/,""); print; exit}' Cargo.toml)}"
[ -n "$VERSION" ] || { echo "error: no [workspace.package] version in Cargo.toml" >&2; exit 1; }
case "$(uname -m)" in
  x86_64) ARCH=x64 ;;
  aarch64 | arm64) ARCH=arm64 ;;
  *) ARCH="$(uname -m)" ;;
esac
DIST="${DIST:-$ROOT/dist/release}"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
ID="io.github.r_dvl.rewind_os"
NAME="rewind-os-$VERSION-linux-$ARCH"

echo "Rewind OS $VERSION for Linux $ARCH"
if [ "${1:-}" != "--skip-build" ]; then
  cargo build --release --locked -p rewind-os -p rewind-os-cli
fi

STAGE="$TARGET_DIR/linux-package/$NAME"
rm -rf "$STAGE"
mkdir -p "$STAGE/bin" "$STAGE/share/applications" "$DIST"
install -m 0755 "$TARGET_DIR/release/rewind-os" "$TARGET_DIR/release/rewind-os-cli" "$STAGE/bin/"
install -m 0644 "packaging/linux/$ID.desktop" "$STAGE/share/applications/"
mkdir -p "$STAGE/share/icons"
cp -r assets/app-icon/hicolor "$STAGE/share/icons/hicolor"
install -m 0644 README.md README.es.md LICENSE-MIT LICENSE-APACHE NOTICE "$STAGE/"

# Both binaries must be for this machine and run.
for bin in rewind-os rewind-os-cli; do
  [ "$(head -c 4 "$STAGE/bin/$bin" | od -An -c | tr -d ' ')" = '177ELF' ] ||
    { echo "error: $bin is not an ELF binary" >&2; exit 1; }
done
"$STAGE/bin/rewind-os-cli" --version
"$STAGE/bin/rewind-os-cli" --json overview >/dev/null

tar -C "$(dirname "$STAGE")" -czf "$DIST/$NAME.tar.gz" "$NAME"
(cd "$DIST" && sha256sum "$NAME.tar.gz" > "$NAME.tar.gz.sha256")
ls -l "$DIST/$NAME.tar.gz"
