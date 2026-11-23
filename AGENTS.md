# Rules for people and agents working on ReWindows

These rules adapt the development standards of the Craft apps
([storytold/craft-anything](https://github.com/storytold/craft-anything), MIT OR Apache-2.0) to this
project. MUST, SHOULD and MAY are used as in RFC 2119. When you depart from a SHOULD, say why in the
pull request.

## What this is

ReWindows is a native Windows 11 app, written only in Rust, that shows what uses disk space and
removes what is not needed: temporary files, caches, leftovers of uninstalled programs, large
forgotten files, and unneeded startup programs. It ships a desktop app (egui) and a CLI, both
driven by one engine.

## Layout

Layers, lowest first. A crate MUST NOT depend on a crate in a higher layer.

| Layer | Crate | Job |
|---|---|---|
| L0 | `crates/i18n` | `tr()` / `trf()`; English text is the key, catalogs in `locales/*.tsv` |
| L0 | `crates/platform` | Everything that talks to Windows: registry, processes, elevation, recycle bin |
| L1 | `crates/engine` | Every feature, as a `Request` with a stable tool id (`TOOLS` in `lib.rs`) |
| L2 | `crates/ui` | The egui interface; the only crate that uses egui, eframe or rfd |
| app | `apps/rewindows` | Desktop executable (icon, manifest) |
| app | `apps/rewindows-cli` | Command line; `rewindows-cli commands` lists every tool |

## Changing code

1. Every user-visible action MUST be an engine `Request` handled in `Engine::run`. Panels change the
   system only through it, so the CLI (and future automation) can do everything the GUI can.
2. Apps MUST be Rust only: no handwritten JavaScript or TypeScript, no webview.
3. `unsafe_code` is forbidden workspace-wide. Windows APIs are reached through safe crates
   (`winreg`, `trash`, `sysinfo`) or Windows' own programs (`whoami`, `dism`, `powercfg`).
4. Non-test code MUST NOT panic: no `unwrap`, `expect`, `panic!` or indexing that can fail.
   Fix what a lint reports; do not silence it with `allow` or a lint-config change.
5. Dates use `chrono`. Locks use `parking_lot::Mutex`.
6. Colours, radii and spacing come from the theme tokens in `crates/ui/src/theme.rs`.
7. User-facing text goes through `tr()` with English as the key. Add the Spanish line to
   `crates/i18n/locales/es.tsv` in the same change; a missing translation shows English.
8. Never present partial or unsupported behaviour as success. Files that could not be deleted,
   categories skipped for lack of rights and stopped scans are reported, in the GUI and the CLI.
9. Anything that deletes or changes the system MUST ask first (a confirmation dialog in the GUI,
   `--yes` in the CLI). User files go to the recycle bin; only regenerable caches are deleted
   permanently.
10. Never follow symbolic links or junctions when measuring or deleting.
11. A UI change SHOULD work by keyboard. Drawn surfaces (bars, treemap tiles) expose a role, a
    label and a value through `widget_info`. Colour is never the only signal: badges carry text.
12. A new dependency's licence MUST be accepted by `deny.toml`.
13. Keep secrets and personal details out of code, logs, fixtures and requests.

## Tests

- A test guards a behaviour a plausible change could break. Do not test constants, wiring or
  library behaviour. Assert relations ("inside", "larger than", "kept") rather than chosen values.
- Integration tests are one binary per crate: `tests/it/main.rs`.
- Never delete, weaken or skip a test to make a check pass.
- UI checks: `REWINDOWS_PAGE=<page> REWINDOWS_SCREENSHOT=out.ppm cargo run -p rewindows` saves the
  window and exits; `REWINDOWS_SCAN=<path>` starts a scan first. See `crates/ui/src/devtools.rs`.

## Checks

Run for the crates you changed, and say in the pull request what ran and what did not:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings
cargo test --workspace
```

## Conventions

- Branches: `feature/`, `bugfix/`, `refactor/`, `docs/`, `test/` or `chore/`, then the roadmap id
  (`feature/r1.2-duplicates`). Pull requests are squash-merged; the title is the commit message.
- One pull request per change. Moving code and changing behaviour go in separate pull requests.
- The plan is [docs/ROADMAP.md](docs/ROADMAP.md); known traps are in [docs/pitfalls.md](docs/pitfalls.md).

## Releases

- The version lives only in `[workspace.package] version` in `Cargo.toml`. A suffix
  (`0.2.0-rc.1`) makes a pre-release.
- Pushing to the `release` branch runs `.github/workflows/release.yml`: it builds x64 and arm64
  with `packaging/windows/package.ps1`, drafts the GitHub release `v<version>` with every package
  and `SHA256SUMS.txt`, and publishes it only when every build passed and no file is missing
  compared with the previous release. Running the workflow by hand is a dry run (artifacts only).
- Publishing a release cannot be undone by a new commit: ask the maintainer before pushing to
  `release`. A CI or release workflow change goes in its own pull request.
- Signing is optional for now: set the `WINDOWS_CERTIFICATE` (base64 `.pfx`) and
  `WINDOWS_CERTIFICATE_PASSWORD` secrets to sign. Unsigned builds say so in the run summary.
