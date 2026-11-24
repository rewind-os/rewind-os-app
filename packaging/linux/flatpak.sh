#!/usr/bin/env bash
# Build the Flatpak bundle from the binaries packaging/linux/package.sh made:
#
#   rewind-os-<version>-linux-<arch>.flatpak   install with: flatpak install --user ./<file>
#
# Needs flatpak and flatpak-builder; the runtime is installed from Flathub when missing.
#   packaging/linux/flatpak.sh
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

VERSION="${REWIND_OS_VERSION:-$(awk '/^\[workspace.package\]/{p=1;next} /^\[/{p=0} p && /^version *=/{gsub(/.*= *"|".*/,""); print; exit}' Cargo.toml)}"
case "$(uname -m)" in
  x86_64) ARCH=x64 ;;
  aarch64 | arm64) ARCH=arm64 ;;
  *) ARCH="$(uname -m)" ;;
esac
DIST="${DIST:-$ROOT/dist/release}"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
ID="io.github.rewind_os.rewind_os_app"
MANIFEST="packaging/linux/flatpak/$ID.yml"
[ -x "$TARGET_DIR/release/rewind-os" ] || { echo "error: run packaging/linux/package.sh first" >&2; exit 1; }

flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user --noninteractive flathub org.freedesktop.Platform//24.08 org.freedesktop.Sdk//24.08

WORK="$TARGET_DIR/flatpak"
rm -rf "$WORK"
mkdir -p "$WORK" "$DIST"
flatpak-builder --user --force-clean --disable-rofiles-fuse --repo="$WORK/repo" "$WORK/build" "$MANIFEST"
BUNDLE="rewind-os-$VERSION-linux-$ARCH.flatpak"
flatpak build-bundle "$WORK/repo" "$DIST/$BUNDLE" "$ID"
(cd "$DIST" && sha256sum "$BUNDLE" > "$BUNDLE.sha256")
ls -l "$DIST/$BUNDLE"
