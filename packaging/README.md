# Packaging

- `windows/package.ps1`: the portable zip and the MSI, for x64 and ARM64.
- `linux/package.sh` and `linux/flatpak.sh`: the tar.gz, .deb, .rpm and Flatpak bundle.
- `macos/package.sh`: a universal app (Apple silicon and Intel) in a .dmg, signed ad hoc. Signing
  with an Apple Developer ID and notarizing need a paid Apple Developer account; until then macOS
  asks to confirm the app the first time it opens.
- `linux/repos.sh`: the signed apt, dnf and zypper repositories on GitHub Pages, from a release's
  .deb and .rpm packages; `linux/check-repos.sh` installs from them in clean containers.
- `minisign.pub`: the public key that signs every release.

## Release signature

The release workflow lists the SHA-256 of every package in `SHA256SUMS.txt` and signs that file
with [minisign](https://jedisct1.github.io/minisign/) (Ed25519) as `SHA256SUMS.txt.minisig`.
The signature's trusted comment is `rewind-os <version> SHA256SUMS.txt`, so a signature cannot be
passed off as another version's. The app's updater installs a package only when its checksum is
in a `SHA256SUMS.txt` whose signature checks out against this key and names the version it
expects. The workflow stops when the secret key is missing or does not match `minisign.pub`.

To check a download by hand:

```sh
minisign -V -p minisign.pub -m SHA256SUMS.txt
sha256sum --check --ignore-missing SHA256SUMS.txt
```

### The key

The secret key lives only in the `MINISIGN_SECRET_KEY` repository secret and in the
maintainer's offline backup. It has no password, because the workflow signs without a person
present; the secret store protects it. To create it (once):

```sh
minisign -G -W -p packaging/minisign.pub -s rewind-os.key
gh secret set MINISIGN_SECRET_KEY < rewind-os.key
```

Then keep `rewind-os.key` somewhere safe outside the repository (a password manager) and delete
the copy on disk. If it is lost, installed apps cannot trust new releases until a release signed
with the old key has shipped a new public key, so do not lose it. If it leaks, publish a release
with a new key at once and say so in its notes.

## Package repositories (apt, dnf, zypper)

After a release is published (not a pre-release or a dry run), the release workflow puts its
.deb and .rpm packages in signed repositories and deploys them to GitHub Pages at
<https://rewind-os.github.io/rewind-os-app/>, so Linux installs and updates Rewind OS with its own
package manager:

```sh
# Debian, Ubuntu
curl -fsSL https://rewind-os.github.io/rewind-os-app/rewind-os.gpg | sudo tee /usr/share/keyrings/rewind-os.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/rewind-os.gpg] https://rewind-os.github.io/rewind-os-app stable main" | sudo tee /etc/apt/sources.list.d/rewind-os.list
sudo apt update && sudo apt install rewind-os
# Fedora
sudo curl -fsSL -o /etc/yum.repos.d/rewind-os.repo https://rewind-os.github.io/rewind-os-app/rpm/rewind-os.repo
sudo dnf install rewind-os
# openSUSE
sudo zypper addrepo https://rewind-os.github.io/rewind-os-app/rpm/rewind-os.repo
sudo zypper install rewind-os
```

The repositories hold the newest version only. One GPG key, kept in the `APT_SIGNING_KEY`
secret, signs the APT `InRelease` and `Release.gpg`, every .rpm and the RPM `repomd.xml`. Every
run of the workflow, dry runs included, builds them and installs from them in clean Debian 12,
Ubuntu 24.04, Fedora and openSUSE Tumbleweed containers (`linux/check-repos.sh`); only a
published release deploys them. Without the secret nothing is built or deployed, the run
summary says so, and the release is not held back.

Setting it up (once):

1. Create the key, without a passphrase (the workflow signs without a person present), and
   store it:

   ```sh
   gpg --batch --passphrase '' --quick-gen-key "Rewind OS packages" ed25519 sign never
   gpg --armor --export-secret-keys "Rewind OS packages" | gh secret set APT_SIGNING_KEY
   ```

   Keep a copy of the secret key somewhere safe outside the repository. Changing it later means
   every computer has to fetch `rewind-os.gpg` again.
2. Settings → Pages → Source: **GitHub Actions**.
3. Settings → Environments → `github-pages` → Deployment branches: add `release`, the branch
   the release workflow runs on.
