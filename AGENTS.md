# Rules for people and agents working on Rewind OS

These rules adapt the development standards of the Craft apps
([storytold/craft-anything](https://github.com/storytold/craft-anything), MIT OR Apache-2.0) to this
project. MUST, SHOULD and MAY are used as in RFC 2119. When you depart from a SHOULD, say why in the
pull request.

## What this is

Rewind OS is a native app, written only in Rust, that shows what uses disk space and removes
what is not needed: temporary files, caches, leftovers of uninstalled programs, large forgotten
files, and unneeded startup programs. Windows comes first; Linux and macOS share the engine and
grow tool by tool. It ships a desktop app (egui) and a CLI, both driven by one engine. Display
name **Rewind OS**; machine names `rewind-os-*`; app id `io.github.rewind_os.rewind_os_app`.

## Layout

Layers, lowest first. A crate MUST NOT depend on a crate in a higher layer.

| Layer | Crate | Job |
|---|---|---|
| L0 | `crates/i18n` | `tr()` / `trf()`; English text is the key, catalogs in `locales/*.tsv` |
| L0 | `crates/platform` | Everything that talks to the OS: known folders, registry, processes, elevation, recycle bin |
| L1 | `crates/engine` | Every feature, as a `Request` with a stable tool id (`TOOLS` in `lib.rs`) |
| L2 | `crates/ui` | The egui interface; the only crate that uses egui, eframe or rfd |
| L2 | `crates/tui` | The terminal interface (`rewind-os-cli tui`); the only crate that uses ratatui |
| app | `apps/rewind-os` | Desktop executable (icon, manifest) |
| app | `apps/rewind-os-cli` | Command line; `rewind-os-cli commands` lists every tool |
| tool | `xtask` | `cargo xtask icons` renders the app icon files from `assets/app-icon/rewind-os.svg` |

## Changing code

1. Every user-visible action MUST be an engine `Request` handled in `Engine::run`. Panels change the
   system only through it, so the CLI (and future automation) can do everything the GUI can.
2. Apps MUST be Rust only: no handwritten JavaScript or TypeScript, no webview.
3. Every tool in `TOOLS` lists the systems it works on (`.only(...)`). Elsewhere `Engine::run`
   answers `EngineError::Unsupported`, the GUI greys the page out and says why, and the CLI says
   so. Platform differences live in `crates/platform` or behind `Os::current()`, not in panels.
   Cleanup categories are per system (`cleanup::WINDOWS`, `LINUX`, `MACOS`); a shared idea keeps
   the same id on every system.
4. `unsafe_code` is forbidden workspace-wide. Windows APIs are reached through safe crates
   (`winreg`, `trash`, `sysinfo`) or Windows' own programs (`whoami`, `dism`, `powercfg`).
5. Non-test code MUST NOT panic: no `unwrap`, `expect`, `panic!` or indexing that can fail.
   Fix what a lint reports; do not silence it with `allow` or a lint-config change.
6. Dates use `chrono`. Locks use `parking_lot::Mutex`.
7. Colours, radii and spacing come from the theme tokens in `crates/ui/src/theme.rs`. The look
   follows Termius: deep navy layers (the page, then the navigation, the page's tool band and
   the cards a step lighter, then hovered and selected items; white and cool greys in the light
   theme), one blue for actions and selection, flat surfaces without borders, names and buttons
   in semibold, list rows as cards (items people pick from, such as programs, as a grid of cards that opens
   a detail panel on the right), icons as white line glyphs (`crates/ui/src/glyphs.rs`) on a
   rounded square of a `TILES` colour, tags as words on a faint tint, muted secondary text.
   Figures are shown as charts from `crates/ui/src/charts.rs` (key-figure tiles, gauges, ring
   charts, stacked bars) in the theme's categorical `palette`, always next to their numbers. Add
   no new colours, outlines or decorations without a reason.
8. User-facing text goes through `tr()` with English as the key. Add the Spanish line to
   `crates/i18n/locales/es.tsv` in the same change; a missing translation shows English.
9. Never present partial or unsupported behaviour as success. Files that could not be deleted,
   categories skipped for lack of rights and stopped scans are reported, in the GUI and the CLI.
10. Safety first: the app must never leave a computer unable to start or wipe personal
   files. Every removal MUST go through `crates/engine/src/safety.rs`; widening it needs a test
   that shows the new place is safe. Read [docs/safety.md](docs/safety.md) before changing
   cleanup, removal or startup code.
11. Anything that deletes, or changes the system in a way the app cannot undo, MUST ask first (a
   confirmation dialog in the GUI); every change needs `--yes` in the CLI. Toggles the app can
   undo in place (startup entries, single "lighter Windows" settings) act on click. User files go to the recycle bin; only regenerable caches are deleted
   permanently.
12. Never follow symbolic links or junctions when measuring or deleting.
13. A UI change SHOULD work by keyboard. Drawn surfaces (bars, treemap tiles) expose a role, a
    label and a value through `widget_info`. Colour is never the only signal: badges carry text.
14. A new dependency's licence MUST be accepted by `deny.toml`.
15. Keep secrets and personal details out of code, logs, fixtures and requests.

## Tests

- A test guards a behaviour a plausible change could break. Do not test constants, wiring or
  library behaviour. Assert relations ("inside", "larger than", "kept") rather than chosen values.
- Integration tests are one binary per crate: `tests/it/main.rs`.
- Never delete, weaken or skip a test to make a check pass.
- UI checks: `REWIND_OS_PAGE=<page> REWIND_OS_SCREENSHOT=out.ppm cargo run -p rewind-os` saves the
  window and exits; `REWIND_OS_SCAN=<path>` starts a scan first;
  `REWIND_OS_THEME=light` checks the light theme; `REWIND_OS_VIEW=changes` opens an analyzer view. See `crates/ui/src/devtools.rs`.

## Checks

Run for the crates you changed, and say in the pull request what ran and what did not:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings
cargo clippy --workspace --all-targets --target aarch64-apple-darwin -- -D warnings
cargo test --workspace
cargo deny check licenses bans sources
```

## Conventions

- Branches: `feature/`, `bugfix/`, `refactor/`, `docs/`, `test/` or `chore/`, then the roadmap id
  (`feature/r1.2-duplicates`). Pull requests are squash-merged; the title is the commit message.
- One pull request per change. Moving code and changing behaviour go in separate pull requests.
- The plan is [docs/ROADMAP.md](docs/ROADMAP.md); known traps are in [docs/pitfalls.md](docs/pitfalls.md).

## Releases

- The version lives only in `[workspace.package] version` in `Cargo.toml`. A suffix
  (`0.2.0-rc.1`) makes a pre-release.
- Pushing to the `release` branch runs `.github/workflows/release.yml`: it builds Windows and
  Linux, x64 and arm64, and a universal macOS app, with `packaging/windows/package.ps1`,
  `packaging/linux/package.sh` and `packaging/macos/package.sh`, drafts the GitHub release `v<version>` with every package
  and `SHA256SUMS.txt`, and publishes it only when every build passed and no file is missing
  compared with the previous release. Running the workflow by hand is a dry run (artifacts only).
- Publishing a release cannot be undone by a new commit: ask the maintainer before pushing to
  `release`. A CI or release workflow change goes in its own pull request.
- `SHA256SUMS.txt` is signed with minisign (`SHA256SUMS.txt.minisig`); the release stops without
  the `MINISIGN_SECRET_KEY` secret or when it does not match `packaging/minisign.pub`. See
  [packaging/README.md](packaging/README.md). Changing that key needs a release signed with the old one.
- Code signing is optional for now: set the `WINDOWS_CERTIFICATE` (base64 `.pfx`) and
  `WINDOWS_CERTIFICATE_PASSWORD` secrets to sign. Unsigned builds say so in the run summary.
