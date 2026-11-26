#!/usr/bin/env bash
# Install Rewind OS from the repositories repos.sh built, the way people will, in clean
# containers: apt on Debian and Ubuntu, dnf on Fedora, zypper on openSUSE. Each checks the
# signatures with the key the repository hands out, installs without recommended packages
# (what a computer without a desktop gets) and runs rewind-os-cli.
#
#   packaging/linux/check-repos.sh <output folder of repos.sh> <the URL it was built for>
#
# Serves the folder on 127.0.0.1 and points the repository files there instead of the URL.
# Needs Docker and python3. Checks the x86-64 packages only.
set -euo pipefail

SITE="${1:?usage: check-repos.sh <site folder> <its URL>}"
URL="${2:?usage: check-repos.sh <site folder> <its URL>}"
PORT="${PORT:-8765}"
LOCAL="http://127.0.0.1:$PORT"
SITE="$(cd "$SITE" && pwd)"

WORK="$(mktemp -d)"
python3 -m http.server "$PORT" --bind 127.0.0.1 --directory "$SITE" >/dev/null 2>&1 &
SERVER=$!
trap 'kill "$SERVER" 2>/dev/null || true; rm -rf "$WORK"' EXIT
for _ in $(seq 50); do curl -fsS "$LOCAL/rewind-os.asc" >/dev/null 2>&1 && break; sleep 0.2; done

sed "s#$URL#$LOCAL#g" "$SITE/rpm/rewind-os.repo" > "$WORK/rewind-os.repo"
echo "deb [signed-by=/usr/share/keyrings/rewind-os.gpg] $LOCAL stable main" > "$WORK/rewind-os.list"
cp "$SITE/rewind-os.gpg" "$WORK/rewind-os.gpg"

run() {
  local image="$1" script="$2"
  echo "== $image"
  docker run --rm --network host -v "$WORK:/repo:ro" "$image" sh -euc "$script"
}

APT='
export DEBIAN_FRONTEND=noninteractive
cp /repo/rewind-os.gpg /usr/share/keyrings/rewind-os.gpg
cp /repo/rewind-os.list /etc/apt/sources.list.d/rewind-os.list
apt-get update -qq
apt-get install -y -qq --no-install-recommends rewind-os >/dev/null
rewind-os-cli --version'
run debian:12 "$APT"
run ubuntu:24.04 "$APT"

run fedora:latest '
cp /repo/rewind-os.repo /etc/yum.repos.d/rewind-os.repo
dnf -y -q --setopt=install_weak_deps=False install rewind-os
rewind-os-cli --version'

run opensuse/tumbleweed '
cp /repo/rewind-os.repo /etc/zypp/repos.d/rewind-os.repo
zypper --non-interactive --quiet --gpg-auto-import-keys install --no-recommends rewind-os
rewind-os-cli --version'

echo "apt, dnf and zypper installed Rewind OS from $SITE."
