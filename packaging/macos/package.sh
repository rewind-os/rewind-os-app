#!/usr/bin/env bash
# Build and package Rewind OS for macOS: one universal app (Apple silicon and Intel) in a disk
# image.
#
#   rewind-os-<version>-macos-universal.dmg
#     Rewind OS.app/Contents/
#       Info.plist
#       MacOS/rewind-os, MacOS/rewind-os-cli   (the command-line tool sits next to the app, where
#                                              it asks for the password for one action)
#       Resources/rewind-os.icns
#     Applications -> /Applications
#     README.md, README.es.md, LICENSE-MIT, LICENSE-APACHE, NOTICE
#
# The app is signed ad hoc, not with an Apple Developer ID, and not notarized: the first time,
# macOS asks to confirm it (Control-click the app, Open; or System Settings, Privacy & Security,
# Open Anyway). Runs on macOS only: it needs lipo, sips, iconutil, codesign and hdiutil.
#
#   packaging/macos/package.sh [--skip-build]
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

VERSION="${REWIND_OS_VERSION:-$(awk '/^\[workspace.package\]/{p=1;next} /^\[/{p=0} p && /^version *=/{gsub(/.*= *"|".*/,""); print; exit}' Cargo.toml)}"
[ -n "$VERSION" ] || { echo "error: no [workspace.package] version in Cargo.toml" >&2; exit 1; }
DIST="${DIST:-$ROOT/dist/release}"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
NAME="rewind-os-$VERSION-macos-universal"
# Bundle identifiers allow letters, digits, "-" and "." only, so the app id's "_" become "-".
BUNDLE_ID="io.github.rewind-os.rewind-os-app"
TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
BINS=(rewind-os rewind-os-cli)
# The oldest macOS Rust's Apple silicon target supports.
MIN_MACOS="11.0"

echo "Rewind OS $VERSION for macOS (universal)"
if [ "${1:-}" != "--skip-build" ]; then
  for target in "${TARGETS[@]}"; do
    MACOSX_DEPLOYMENT_TARGET="$MIN_MACOS" \
      cargo build --release --locked --target "$target" -p rewind-os -p rewind-os-cli
  done
fi

STAGE="$TARGET_DIR/macos-package/$NAME"
APP="$STAGE/Rewind OS.app"
rm -rf "$STAGE"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$DIST"

# One binary for both kinds of Mac.
for bin in "${BINS[@]}"; do
  lipo -create -output "$APP/Contents/MacOS/$bin" \
    "$TARGET_DIR/aarch64-apple-darwin/release/$bin" \
    "$TARGET_DIR/x86_64-apple-darwin/release/$bin"
  lipo "$APP/Contents/MacOS/$bin" -verify_arch arm64 x86_64 ||
    { echo "error: $bin is not universal" >&2; exit 1; }
done

# The icon, from the macOS artwork (Apple's transparent margin around the tile).
ICONSET="$TARGET_DIR/macos-package/rewind-os.iconset"
rm -rf "$ICONSET"
mkdir -p "$ICONSET"
SOURCE="assets/app-icon/rewind-os-macos-512.png"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$SOURCE" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  if [ "$double" -le 512 ]; then
    sips -z "$double" "$double" "$SOURCE" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
  fi
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/rewind-os.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Rewind OS</string>
  <key>CFBundleDisplayName</key><string>Rewind OS</string>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleExecutable</key><string>rewind-os</string>
  <key>CFBundleIconFile</key><string>rewind-os</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleLocalizations</key><array><string>en</string><string>es</string></array>
  <key>LSMinimumSystemVersion</key><string>$MIN_MACOS</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSHumanReadableCopyright</key><string>MIT OR Apache-2.0</string>
</dict>
</plist>
PLIST
plutil -lint "$APP/Contents/Info.plist"

# Signed ad hoc: the inner tool first, then the bundle that seals it.
codesign --force --sign - --timestamp=none "$APP/Contents/MacOS/rewind-os-cli"
codesign --force --sign - --timestamp=none "$APP"
codesign --verify --strict --verbose=2 "$APP"

# The tool must run from inside the bundle.
"$APP/Contents/MacOS/rewind-os-cli" --version
"$APP/Contents/MacOS/rewind-os-cli" --json overview >/dev/null

install -m 0644 README.md README.es.md LICENSE-MIT LICENSE-APACHE NOTICE "$STAGE/"
ln -s /Applications "$STAGE/Applications"

rm -f "$DIST/$NAME.dmg"
hdiutil create -volname "Rewind OS $VERSION" -srcfolder "$STAGE" -fs HFS+ -format UDZO \
  -ov "$DIST/$NAME.dmg" >/dev/null
hdiutil verify "$DIST/$NAME.dmg" >/dev/null
(cd "$DIST" && shasum -a 256 "$NAME.dmg" > "$NAME.dmg.sha256")
ls -l "$DIST/$NAME.dmg"
