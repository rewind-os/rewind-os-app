#!/usr/bin/env bash
# Build and package Rewind OS for Linux: a portable tar.gz, a .deb and (when rpmbuild is
# installed) an .rpm, all from the same two binaries.
#
#   rewind-os-<version>-linux-<arch>.tar.gz
#     rewind-os-<version>-linux-<arch>/
#       bin/rewind-os, bin/rewind-os-cli
#       share/applications/io.github.rewind_os.rewind_os_app.desktop
#       share/icons/hicolor/<n>x<n>/apps/io.github.rewind_os.rewind_os_app.png
#       README.md, README.es.md, LICENSE-MIT, LICENSE-APACHE, NOTICE
#
# Unpack it anywhere and run bin/rewind-os; copy share/ into ~/.local/share to get the
# launcher entry and icon.
#
#   rewind-os_<version>_<amd64|arm64>.deb, rewind-os-<version>-1.<x86_64|aarch64>.rpm
#     /usr/bin/rewind-os, /usr/bin/rewind-os-cli, the launcher entry, the icons, the licences
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
ID="io.github.rewind_os.rewind_os_app"
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

# ---------------------------------------------------------------------------------------------
# Native packages. Both install the same files under /usr.
# Package versions cannot contain "-": a pre-release 0.6.0-rc.1 becomes 0.6.0~rc.1, which sorts
# before 0.6.0 in both dpkg and rpm.
PKG_VERSION="${VERSION//-/\~}"
ROOTFS="$TARGET_DIR/linux-package/rootfs"
rm -rf "$ROOTFS"
mkdir -p "$ROOTFS/usr/bin" "$ROOTFS/usr/share/applications" "$ROOTFS/usr/share/icons" \
  "$ROOTFS/usr/share/doc/rewind-os"
install -m 0755 "$STAGE/bin/rewind-os" "$STAGE/bin/rewind-os-cli" "$ROOTFS/usr/bin/"
install -m 0644 "packaging/linux/$ID.desktop" "$ROOTFS/usr/share/applications/"
cp -r assets/app-icon/hicolor "$ROOTFS/usr/share/icons/hicolor"
install -m 0644 README.md LICENSE-MIT LICENSE-APACHE NOTICE "$ROOTFS/usr/share/doc/rewind-os/"
SUMMARY="See what uses your disk and remove what you do not need"
DESCRIPTION="Rewind OS shows what uses disk space and removes what is not needed: temporary
files, caches, leftovers of uninstalled programs, duplicate and large forgotten files, and
unneeded startup programs. Every removal goes through a safety guard."
HOMEPAGE="https://github.com/rewind-os/rewind-os-app"
# The GUI loads OpenGL, xkbcommon and the display libraries at run time.
DEB_DEPENDS="libc6, libgl1, libxkbcommon0, libwayland-client0 | libx11-6"

case "$ARCH" in
  x64) DEB_ARCH=amd64; RPM_ARCH=x86_64 ;;
  arm64) DEB_ARCH=arm64; RPM_ARCH=aarch64 ;;
  *) DEB_ARCH="$ARCH"; RPM_ARCH="$ARCH" ;;
esac

# .deb
DEB_ROOT="$TARGET_DIR/linux-package/deb"
rm -rf "$DEB_ROOT"
cp -a "$ROOTFS" "$DEB_ROOT"
mkdir -p "$DEB_ROOT/DEBIAN"
{
  echo "Package: rewind-os"
  echo "Version: $PKG_VERSION"
  echo "Architecture: $DEB_ARCH"
  echo "Maintainer: Rewind OS maintainers <rewind-os@users.noreply.github.com>"
  echo "Installed-Size: $(du -sk "$ROOTFS" | cut -f1)"
  echo "Depends: $DEB_DEPENDS"
  echo "Recommends: pkexec | policykit-1"
  echo "Section: utils"
  echo "Priority: optional"
  echo "Homepage: $HOMEPAGE"
  echo "Description: $SUMMARY"
  sed 's/^/ /' <<< "$DESCRIPTION"
} > "$DEB_ROOT/DEBIAN/control"
DEB="rewind-os_${PKG_VERSION}_${DEB_ARCH}.deb"
dpkg-deb --root-owner-group --build "$DEB_ROOT" "$DIST/$DEB" >/dev/null
dpkg-deb --info "$DIST/$DEB" | grep -E "Package|Version|Architecture"
DEB_FILES="$(dpkg-deb --contents "$DIST/$DEB")"
grep -q "usr/bin/rewind-os-cli" <<< "$DEB_FILES" ||
  { echo "error: the .deb does not contain rewind-os-cli" >&2; exit 1; }
(cd "$DIST" && sha256sum "$DEB" > "$DEB.sha256")

# .rpm
if command -v rpmbuild >/dev/null; then
  RPM_TOP="$TARGET_DIR/linux-package/rpmbuild"
  rm -rf "$RPM_TOP"
  mkdir -p "$RPM_TOP"/{SPECS,RPMS,BUILD,BUILDROOT}
  cat > "$RPM_TOP/SPECS/rewind-os.spec" <<SPEC
Name: rewind-os
Version: $PKG_VERSION
Release: 1
Summary: $SUMMARY
License: MIT OR Apache-2.0
URL: $HOMEPAGE
Requires: libglvnd-glx, libxkbcommon
Recommends: polkit
AutoReqProv: no

%description
$DESCRIPTION

%install
mkdir -p %{buildroot}
cp -a $ROOTFS/. %{buildroot}/

%files
/usr/bin/rewind-os
/usr/bin/rewind-os-cli
/usr/share/applications/$ID.desktop
/usr/share/icons/hicolor/*/apps/$ID.*
%doc /usr/share/doc/rewind-os
SPEC
  rpmbuild --define "_topdir $RPM_TOP" --target "$RPM_ARCH" -bb "$RPM_TOP/SPECS/rewind-os.spec" >/dev/null
  RPM_FILE="$(find "$RPM_TOP/RPMS" -name '*.rpm' | head -n 1)"
  [ -n "$RPM_FILE" ] || { echo "error: rpmbuild made no package" >&2; exit 1; }
  cp "$RPM_FILE" "$DIST/"
  RPM="$(basename "$RPM_FILE")"
  (cd "$DIST" && sha256sum "$RPM" > "$RPM.sha256")
  ls -l "$DIST/$RPM"
else
  echo "rpmbuild is not installed: no .rpm (install the rpm package to build one)"
fi
ls -l "$DIST/$DEB"
