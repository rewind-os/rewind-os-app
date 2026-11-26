#!/usr/bin/env bash
# Build the signed APT repository that GitHub Pages serves, from a release's .deb packages, so
# Debian and Ubuntu can `sudo apt install rewind-os` and get updates with `apt upgrade`.
#
#   packaging/linux/apt-repo.sh <folder with the .deb files> <output folder>
#
# The output holds the newest packages only (apt installs the newest anyway):
#
#   index.html                          how to add the repository
#   rewind-os.gpg, rewind-os.asc        the public key, binary (for signed-by) and armoured
#   pool/main/r/rewind-os/*.deb
#   dists/stable/Release, InRelease, Release.gpg
#   dists/stable/main/binary-<amd64|arm64>/Packages, Packages.gz
#
# Signs with the GPG key whose secret part is in APT_SIGNING_KEY (armoured). The key stays in
# a temporary GnuPG home that is removed on exit. Needs apt-ftparchive (apt-utils) and gpg.
set -euo pipefail

DEBS="${1:?usage: apt-repo.sh <folder with .deb files> <output folder>}"
OUT="${2:?usage: apt-repo.sh <folder with .deb files> <output folder>}"
: "${APT_SIGNING_KEY:?the APT_SIGNING_KEY environment variable must hold the armoured secret key}"
ARCHES=(amd64 arm64)
SUITE=stable
URL="${APT_REPO_URL:-https://rewind-os.github.io/rewind-os-app}"

shopt -s nullglob
debs=("$DEBS"/rewind-os_*.deb)
[ "${#debs[@]}" -gt 0 ] || { echo "error: no rewind-os_*.deb in $DEBS" >&2; exit 1; }

GNUPGHOME="$(mktemp -d)"
export GNUPGHOME
trap 'gpgconf --kill gpg-agent 2>/dev/null || true; rm -rf "$GNUPGHOME"' EXIT
chmod 700 "$GNUPGHOME"
printf '%s\n' "$APT_SIGNING_KEY" | gpg --batch --quiet --import
KEY="$(gpg --batch --with-colons --list-secret-keys | awk -F: '$1 == "fpr" { print $10; exit }')"
[ -n "$KEY" ] || { echo "error: APT_SIGNING_KEY holds no secret key" >&2; exit 1; }

rm -rf "$OUT"
POOL="$OUT/pool/main/r/rewind-os"
mkdir -p "$POOL"
cp "${debs[@]}" "$POOL/"

cd "$OUT"
for arch in "${ARCHES[@]}"; do
  dir="dists/$SUITE/main/binary-$arch"
  mkdir -p "$dir"
  apt-ftparchive --arch "$arch" packages pool > "$dir/Packages"
  gzip -9 -k -n "$dir/Packages"
done
apt-ftparchive \
  -o "APT::FTPArchive::Release::Origin=Rewind OS" \
  -o "APT::FTPArchive::Release::Label=Rewind OS" \
  -o "APT::FTPArchive::Release::Suite=$SUITE" \
  -o "APT::FTPArchive::Release::Codename=$SUITE" \
  -o "APT::FTPArchive::Release::Architectures=${ARCHES[*]}" \
  -o "APT::FTPArchive::Release::Components=main" \
  -o "APT::FTPArchive::Release::Description=Rewind OS packages" \
  release "dists/$SUITE" > "dists/$SUITE/Release.tmp"
mv "dists/$SUITE/Release.tmp" "dists/$SUITE/Release"

sign=(gpg --batch --yes --quiet --pinentry-mode loopback --local-user "$KEY" --digest-algo SHA512)
"${sign[@]}" --clearsign --output "dists/$SUITE/InRelease" "dists/$SUITE/Release"
"${sign[@]}" --armor --detach-sign --output "dists/$SUITE/Release.gpg" "dists/$SUITE/Release"
gpg --batch --export "$KEY" > rewind-os.gpg
gpg --batch --armor --export "$KEY" > rewind-os.asc

# The signature must check out against the key the repository hands out.
gpgv --keyring ./rewind-os.gpg "dists/$SUITE/InRelease" 2>/dev/null ||
  { echo "error: InRelease does not verify against rewind-os.gpg" >&2; exit 1; }

cat > index.html <<HTML
<!doctype html>
<html lang="en">
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Rewind OS APT repository</title>
<style>
  body { font: 16px/1.5 system-ui, sans-serif; max-width: 52rem; margin: 2rem auto; padding: 0 1rem; background: #1d2033; color: #e6e8f0; }
  pre { background: #282b3d; padding: 1rem; border-radius: 8px; overflow-x: auto; }
  a { color: #5aa8ff; }
</style>
<h1>Rewind OS for Debian and Ubuntu</h1>
<p>Add the repository once; <code>sudo apt upgrade</code> then keeps Rewind OS up to date.</p>
<pre>curl -fsSL $URL/rewind-os.gpg | sudo tee /usr/share/keyrings/rewind-os.gpg &gt;/dev/null
echo "deb [signed-by=/usr/share/keyrings/rewind-os.gpg] $URL $SUITE main" | sudo tee /etc/apt/sources.list.d/rewind-os.list
sudo apt update
sudo apt install rewind-os</pre>
<p>On a computer without a desktop, <code>sudo apt install --no-install-recommends rewind-os</code> installs what <code>rewind-os-cli</code> needs only.</p>
<p>Key fingerprint: <code>$KEY</code>. <a href="https://github.com/rewind-os/rewind-os-app">Rewind OS on GitHub</a>.</p>
</html>
HTML

echo "APT repository in $OUT, signed by $KEY:"
find . -type f | sort
