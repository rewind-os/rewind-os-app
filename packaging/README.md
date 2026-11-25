# Packaging

- `windows/package.ps1`: the portable zip and the MSI, for x64 and ARM64.
- `linux/package.sh` and `linux/flatpak.sh`: the tar.gz, .deb, .rpm and Flatpak bundle.
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
