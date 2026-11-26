#!/usr/bin/env bash
# Build the signed package repositories that GitHub Pages serves, from a release's .deb and
# .rpm packages, so Linux installs Rewind OS with its own package manager and updates it with
# the rest of the system: apt (Debian, Ubuntu), dnf (Fedora) and zypper (openSUSE).
#
#   packaging/linux/repos.sh <folder with the packages> <output folder>
#
# The output holds the newest packages only (package managers install the newest anyway):
#
#   index.html                          how to add the repositories
#   rewind-os.gpg, rewind-os.asc        the public key, binary (for apt's signed-by) and armoured
#   pool/main/r/rewind-os/*.deb         the APT repository
#   dists/stable/Release, InRelease, Release.gpg
#   dists/stable/main/binary-<amd64|arm64>/Packages, Packages.gz
#   rpm/*.rpm                           the RPM repository, every package signed
#   rpm/repodata/repomd.xml, repomd.xml.asc
#   rpm/rewind-os.repo                  the file dnf and zypper add
#
# Signs with the GPG key whose secret part is in APT_SIGNING_KEY (armoured). The key stays in
# a temporary GnuPG home that is removed on exit. Needs apt-ftparchive (apt-utils), gpg, and
# for the RPM repository rpmsign (rpm) and createrepo_c.
set -euo pipefail

PKGS="${1:?usage: repos.sh <folder with the packages> <output folder>}"
OUT="${2:?usage: repos.sh <folder with the packages> <output folder>}"
: "${APT_SIGNING_KEY:?the APT_SIGNING_KEY environment variable must hold the armoured secret key}"
ARCHES=(amd64 arm64)
SUITE=stable
URL="${APT_REPO_URL:-https://rewind-os.github.io/rewind-os-app}"

shopt -s nullglob
PKGS="$(cd "$PKGS" && pwd)"
debs=("$PKGS"/rewind-os_*.deb)
rpms=("$PKGS"/rewind-os-*.rpm)
[ "${#debs[@]}" -gt 0 ] || { echo "error: no rewind-os_*.deb in $PKGS" >&2; exit 1; }
[ "${#rpms[@]}" -gt 0 ] || { echo "error: no rewind-os-*.rpm in $PKGS" >&2; exit 1; }

GNUPGHOME="$(mktemp -d)"
export GNUPGHOME
trap 'gpgconf --kill gpg-agent 2>/dev/null || true; rm -rf "$GNUPGHOME"' EXIT
chmod 700 "$GNUPGHOME"
printf '%s\n' "$APT_SIGNING_KEY" | gpg --batch --quiet --import
KEY="$(gpg --batch --with-colons --list-secret-keys | awk -F: '$1 == "fpr" { print $10; exit }')"
[ -n "$KEY" ] || { echo "error: APT_SIGNING_KEY holds no secret key" >&2; exit 1; }

rm -rf "$OUT"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
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

# RPM: every package signed (dnf and zypper check each one), then the metadata signed too.
mkdir -p rpm
cp "${rpms[@]}" rpm/
rpmsign --define "_gpg_name $KEY" \
  --define "_gpg_sign_cmd_extra_args --batch --pinentry-mode loopback" \
  --addsign rpm/*.rpm >/dev/null
RPMDB="$(mktemp -d)"
rpm --dbpath "$RPMDB" --import rewind-os.asc
for package in rpm/*.rpm; do
  rpm --dbpath "$RPMDB" --checksig "$package" | grep -q "signatures OK" ||
    { echo "error: $package is not signed by the repository's key" >&2; rm -rf "$RPMDB"; exit 1; }
done
rm -rf "$RPMDB"
createrepo_c --quiet rpm
"${sign[@]}" --armor --detach-sign --output rpm/repodata/repomd.xml.asc rpm/repodata/repomd.xml
cat > rpm/rewind-os.repo <<REPO
[rewind-os]
name=Rewind OS
baseurl=$URL/rpm
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=$URL/rewind-os.asc
REPO

cat > index.html <<HTML
<!doctype html>
<html lang="en">
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Rewind OS package repositories</title>
<style>
  body { font: 16px/1.5 system-ui, sans-serif; max-width: 52rem; margin: 2rem auto; padding: 0 1rem; background: #1d2033; color: #e6e8f0; }
  pre { background: #282b3d; padding: 1rem; border-radius: 8px; overflow-x: auto; }
  a { color: #5aa8ff; }
</style>
<h1>Rewind OS package repositories</h1>
<p>Add the repository once; the system's updates then keep Rewind OS up to date.</p>
<h2>Debian and Ubuntu (apt)</h2>
<pre>curl -fsSL $URL/rewind-os.gpg | sudo tee /usr/share/keyrings/rewind-os.gpg &gt;/dev/null
echo "deb [signed-by=/usr/share/keyrings/rewind-os.gpg] $URL $SUITE main" | sudo tee /etc/apt/sources.list.d/rewind-os.list
sudo apt update
sudo apt install rewind-os</pre>
<h2>Fedora (dnf)</h2>
<pre>sudo curl -fsSL -o /etc/yum.repos.d/rewind-os.repo $URL/rpm/rewind-os.repo
sudo dnf install rewind-os</pre>
<h2>openSUSE (zypper)</h2>
<pre>sudo zypper addrepo $URL/rpm/rewind-os.repo
sudo zypper install rewind-os</pre>
<p>On a computer without a desktop, install without recommended packages (<code>--no-install-recommends</code> with apt and zypper, <code>--setopt=install_weak_deps=False</code> with dnf) to get what <code>rewind-os-cli</code> needs only.</p>
<p>Key fingerprint: <code>$KEY</code>. <a href="https://github.com/rewind-os/rewind-os-app">Rewind OS on GitHub</a>.</p>
</html>
HTML

echo "Repositories in $OUT, signed by $KEY:"
find . -type f | sort
