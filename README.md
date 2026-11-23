# ReWindows

[Español](README.es.md)

A small, fast, native Windows 11 app, written only in Rust, that shows what is using your disk and
removes what you do not need: temporary files, caches, leftovers of programs you uninstalled,
large forgotten files and programs that start with Windows for no reason.

- **Overview**: drives (SSD/HDD), memory, and Windows' own large files (`hiberfil.sys`,
  `pagefile.sys`).
- **Disk analyzer**: scans a drive or folder in parallel; folder list, treemap, largest files and
  size per file type. Anything can be shown in File Explorer or moved to the recycle bin.
- **Junk cleanup**: temporary files, Windows Update downloads, crash dumps, browser and app caches,
  package manager caches, old installers in Downloads, the recycle bin. Each category says how
  safe it is and whether it needs administrator rights.
- **Program leftovers**: folders in Program Files, ProgramData and AppData that no installed
  program, service, running program or startup entry refers to. They go to the recycle bin.
- **Installed programs** with search and their own uninstaller.
- **Startup programs** on and off, using the same records as Windows' settings.
- **Maintenance**: real size and cleanup of the component store (WinSxS), hibernation on/off.

Nothing is deleted without asking. Files a program has open are skipped and reported. The app
starts as a normal user and offers to restart as administrator when a task needs it.

## Build

On Windows 11 with [Rust](https://rustup.rs) and the Visual Studio C++ Build Tools:

```powershell
cargo build --release
.\target\release\rewindows.exe        # desktop app
.\target\release\rewindows-cli.exe    # command line
```

## Command line

Every feature of the app is also a command:

```powershell
rewindows-cli commands                       # every tool and its id
rewindows-cli overview
rewindows-cli scan C:\ --top 30
rewindows-cli clean analyze
rewindows-cli clean run --defaults --yes     # the categories marked safe
rewindows-cli leftovers
rewindows-cli startup list
rewindows-cli --json run '{"tool":"cleanup.analyze"}'
```

`--json` prints the engine's response for scripts. `--lang es` switches the language.

## Development

Rules for contributors and coding agents: [AGENTS.md](AGENTS.md). Plan: [docs/ROADMAP.md](docs/ROADMAP.md).

## Licence

MIT OR Apache-2.0, at your option. See [NOTICE](NOTICE).
