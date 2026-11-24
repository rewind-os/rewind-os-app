<p align="center">
  <img src="assets/app-icon/hicolor/256x256/apps/io.github.rewind_os.rewind_os_app.png" width="128" height="128" alt="Rewind OS icon: an engraved raccoon on a teal tile">
</p>

<h1 align="center">Rewind OS</h1>

<p align="center">
  <a href="README.es.md">Español</a>
</p>

<p align="center">
  <b>See what is using your disk and take your computer back to clean.</b><br>
  A small, fast, native app written only in Rust. Windows first; Linux and macOS on the way.
</p>

It removes what you do not need: temporary files, caches, leftovers of programs you uninstalled,
large forgotten files and programs that start with the system for no reason.

- **Home**: recommendations for this computer (a drive almost full, junk worth cleaning, too
  many startup programs, duplicates in Downloads, leftovers of removed programs…), each with a
  button that does it; drives (SSD/HDD), memory, and Windows' own large files (`hiberfil.sys`,
  `pagefile.sys`).
- **Disk analyzer**: scans a drive or folder in parallel; folder list, treemap, largest files and
  size per file type. Anything can be shown in the file manager or moved to the recycle bin.
- **Duplicate files**: files with exactly the same contents (size, then first block, then a full
  BLAKE3 hash). Hard links are not counted as copies and cloud-only files are not downloaded. One
  copy of each file always stays; the others go to the recycle bin.
- **Junk cleanup**: temporary files, update downloads, crash reports, browser and app caches,
  package manager caches, old installers in Downloads, the recycle bin. Each category says how
  safe it is and whether it needs administrator rights.
- **Program leftovers**: folders in Program Files, ProgramData and AppData that no installed
  program, service, running program or startup entry refers to. They go to the recycle bin.
  Registry keys of removed programs are reported too, never removed.
- **Installed programs** with their icons, search and their own uninstaller.
- **Startup programs** on and off, using the system's own records, including the scheduled tasks
  programs add for themselves and the services they install (Windows' own are never shown;
  services are set to start when needed, never disabled).
- **Make Windows lighter**: turn off ads, suggestions, tracking and extras you do not use. A
  closed list of documented settings, each of which can be undone, and optional removal of
  preinstalled Store apps (News, Tips, Clipchamp…) for your account; the Store can install
  them again.
- **Maintenance**: the system's own tools (component store and hibernation on Windows, the
  journal on Linux).

Nothing is deleted without asking. Files a program has open are skipped and reported. The app
starts as a normal user and, on Windows, offers to restart as administrator when a task needs it.

## Systems

| Feature | Windows | Linux | macOS |
|---|:-:|:-:|:-:|
| Overview, disk analyzer, duplicate files | ✓ | ✓ | ✓ |
| Junk cleanup, recycle bin | ✓ | ✓ | ✓ |
| Startup programs | ✓ | ✓ (XDG autostart) | — |
| Recommendations | ✓ | ✓ | ✓ |
| Installed programs | ✓ | ✓ (APT, pacman, Flatpak, Snap; RPM listed only) | — |
| Program leftovers, make Windows lighter | ✓ | — | — |
| Maintenance | component store, hibernation | journal | — |

A tool that is not available yet says so, in the app and in the CLI; it never pretends to work.
macOS builds are checked in CI but not released yet.

## Download

Ready-made builds are on the [releases page](../../releases); check them against
`SHA256SUMS.txt`. Nothing is installed.

- **Windows** x64 and ARM64: unzip `rewind-os-<version>-windows-<arch>-portable.zip` and run
  `rewind-os.exe`.
- **Linux** x64 and ARM64: unpack `rewind-os-<version>-linux-<arch>.tar.gz` and run
  `bin/rewind-os`. Copy its `share/` folder into `~/.local/share` for the launcher entry and icon.

## Build

With [Rust](https://rustup.rs) (on Windows, also the Visual Studio C++ Build Tools):

```sh
cargo build --release
target/release/rewind-os        # desktop app
target/release/rewind-os-cli    # command line
```

On Linux the build needs the X11/Wayland development headers (`libxkbcommon-dev libwayland-dev
libx11-dev libgl1-mesa-dev` or your distribution's equivalent).

## Command line

Every feature of the app is also a command:

```sh
rewind-os-cli commands                       # every tool and its id
rewind-os-cli overview
rewind-os-cli scan C:\ --top 30
rewind-os-cli duplicates C:\Users\me --min-mb 10
rewind-os-cli clean analyze
rewind-os-cli clean run --defaults --yes     # the categories marked safe
rewind-os-cli leftovers
rewind-os-cli startup list
rewind-os-cli recommend
rewind-os-cli tweaks list
rewind-os-cli tweaks apply --recommended --yes
sudo rewind-os-cli clean run --defaults --yes   # Linux/macOS: system-wide categories too
rewind-os-cli --json run '{"tool":"cleanup.analyze"}'
```

### Assistants (MCP)

`rewind-os-cli mcp` serves the same tools to an assistant over the Model Context Protocol
(stdin/stdout). By default it offers only tools that change nothing; folders and changes are
opened up one flag at a time:

```sh
# Claude Code: read-only, may scan your Downloads folder
claude mcp add rewind-os -- rewind-os-cli mcp --automation-read-root ~/Downloads
# also allow moving files to the recycle bin inside Downloads
rewind-os-cli mcp --automation-write-root ~/Downloads
# also allow cleanup, startup, settings, apps and uninstall tools
rewind-os-cli mcp --allow-system-changes
```

`--json` prints the engine's response for scripts. `--lang es` switches the language.

## Development

Rules for contributors and coding agents: [AGENTS.md](AGENTS.md). Plan: [docs/ROADMAP.md](docs/ROADMAP.md).
The app icon and how to regenerate it: [assets/app-icon/README.md](assets/app-icon/README.md).

## Licence

MIT OR Apache-2.0, at your option. See [NOTICE](NOTICE).
