<p align="center">
  <img src="assets/app-icon/hicolor/256x256/apps/io.github.rewind_os.rewind_os_app.png" width="128" height="128" alt="Rewind OS icon: a rewound cassette tape, its left reel full of blue tape, on a navy tile">
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
  size per file type. Run as administrator on Windows, it reads an NTFS drive's master file table
  in one pass, in seconds. "What changed" names the folders that grew or shrank most since the
  last scan of the same folder (a short summary of folder sizes stays on this computer, in the
  app's data folder). "Old large files" lists files of 50 MB or more that nobody changed in over
  a year, leaving out the system's and programs' own files. "Empty folders" lists folders with
  nothing in them (or only other empty folders), without hidden, system, program or project
  folders, each checked again before it goes. Anything can be shown in the file manager or moved
  to the recycle bin.
- **Duplicate files**: files with exactly the same contents (size, then first block, then a full
  BLAKE3 hash). Hard links are not counted as copies and cloud-only files are not downloaded. One
  copy of each file always stays; the others go to the recycle bin.
- **Build folders**: `node_modules`, Rust's `target`, Python virtual environments, Gradle, Maven,
  .NET, Flutter and CMake output, and web framework caches in your projects. A folder counts only
  beside the file that proves what it is (`package.json` with its lockfile, `Cargo.toml` with
  Cargo's cache tag), never inside hidden folders, app data or programs. Deleted for good, since
  each project makes them again (`npm install`, `cargo build`).
- **Virtual disks** (Windows): the disk files of WSL distributions and Docker Desktop, which grow
  and never shrink by themselves. Each shows what it takes on Windows and, while its distribution
  runs, what Linux uses inside. Compacting one (as administrator, after asking) trims it from
  inside, stops WSL and runs diskpart's `compact vdisk`; nothing inside is deleted.
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
- **Startup speed**: how long the computer takes to start, its last starts, and what slowed them
  down (Windows' own performance log, as administrator; `systemd-analyze` on Linux), with a
  switch for the startup program behind a delay.
- **Running programs**: what each program takes of the processor and memory right now (its
  processes counted together), closing one after asking (forcing only when it does not close),
  and a switch for the ones that start with the system. The system's own programs, other
  accounts' programs and Rewind OS itself are never closed. The power plan is switched on the
  same page (Windows' power mode is shown, and changed in Settings).
- **Disk health**: whether each drive reports problems, with its temperature, wear, hours and
  errors (Windows' own storage reports, the figures as administrator; udisks on Linux), a warning
  on Home when one is failing, and the system's own optimization (TRIM on SSDs, defragmentation
  of hard drives on Windows) run on request.
- **Make Windows lighter**: turn off ads, suggestions, tracking and extras you do not use. A
  closed list of documented settings, each of which can be undone, and optional removal of
  preinstalled Store apps (News, Tips, Clipchamp…) for your account; the Store can install
  them again.
- **Maintenance**: the system's own tools (component store, hibernation and old restore
  points on Windows, the journal on Linux).

Nothing is deleted without asking. Files a program has open are skipped and reported. The app
starts as a normal user and, on Windows, offers to restart as administrator when a task needs it.

## Systems

| Feature | Windows | Linux | macOS |
|---|:-:|:-:|:-:|
| Overview, disk analyzer, duplicate files, build folders | ✓ | ✓ | ✓ |
| Junk cleanup, recycle bin | ✓ | ✓ | ✓ |
| Virtual disks of WSL and Docker | ✓ (compacting as administrator) | — | — |
| Startup programs | ✓ | ✓ (XDG autostart) | ✓ (LaunchAgents) |
| Startup speed | ✓ (as administrator) | ✓ (systemd, last start) | — |
| Running programs | ✓ | ✓ | — |
| Power plan | ✓ | ✓ (power-profiles-daemon) | — |
| Disk health and optimization | ✓ (figures as administrator) | ✓ (udisks; TRIM with the password) | — |
| Recommendations | ✓ | ✓ | ✓ |
| Installed programs | ✓ | ✓ (APT, RPM, pacman, Flatpak, Snap) | ✓ (apps, to the Trash) |
| Program leftovers | ✓ | ✓ (`~/.config`, `~/.local/share`) | ✓ (`~/Library`) |
| Make Windows lighter | ✓ | — | — |
| Maintenance | component store, hibernation, restore points | journal | — |

A tool that is not available yet says so, in the app and in the CLI; it never pretends to work.

## Download

Ready-made builds are on the [releases page](../../releases); check them against
`SHA256SUMS.txt`, which is signed with the key in [packaging/minisign.pub](packaging/minisign.pub)
(see [packaging/README.md](packaging/README.md#release-signature)).

- **Windows** x64 and ARM64: run `rewind-os-<version>-windows-<arch>.msi` to install it (Start
  menu, uninstall from Settings), or unzip `rewind-os-<version>-windows-<arch>-portable.zip` and
  run `rewind-os.exe` without installing.
- **Linux** x64 and ARM64 (glibc 2.35 or newer: Debian 12, Ubuntu 22.04, Fedora 36 and later):
  unpack `rewind-os-<version>-linux-<arch>.tar.gz` and run
  `bin/rewind-os`. Copy its `share/` folder into `~/.local/share` for the launcher entry and icon.
  On Debian and Ubuntu, add the APT repository once and `sudo apt upgrade` keeps it up to date:

  ```sh
  curl -fsSL https://rewind-os.github.io/rewind-os-app/rewind-os.gpg | sudo tee /usr/share/keyrings/rewind-os.gpg >/dev/null
  echo "deb [signed-by=/usr/share/keyrings/rewind-os.gpg] https://rewind-os.github.io/rewind-os-app stable main" | sudo tee /etc/apt/sources.list.d/rewind-os.list
  sudo apt update && sudo apt install rewind-os
  ```

  On Fedora, `sudo curl -fsSL -o /etc/yum.repos.d/rewind-os.repo https://rewind-os.github.io/rewind-os-app/rpm/rewind-os.repo`
  then `sudo dnf install rewind-os`; on openSUSE, `sudo zypper addrepo` with the same file, then
  `sudo zypper install rewind-os`.
  Or install the package: `sudo apt install ./rewind-os_<version>_amd64.deb` (Debian, Ubuntu)
  or `sudo dnf install ./rewind-os-<version>-1.x86_64.rpm` (Fedora, openSUSE); `arm64` and
  `aarch64` for ARM. The window's graphics libraries are recommended, not required, so a
  computer without a desktop can install the package for `rewind-os-cli` alone (with
  `--no-install-recommends`).
  Or install the Flatpak bundle: `flatpak install --user ./rewind-os-<version>-linux-<arch>.flatpak`
  (it needs the host filesystem to see the disk, and runs the system's own tools on the host).
- **macOS** 11 or newer, Apple silicon and Intel: open
  `rewind-os-<version>-macos-universal.dmg` and drag Rewind OS to Applications. The app is not
  signed with an Apple Developer ID yet, so the first time macOS asks to confirm it: Control-click
  the app and choose Open (or System Settings → Privacy & Security → Open Anyway). The
  command-line tool is inside the app: `/Applications/Rewind OS.app/Contents/MacOS/rewind-os-cli`.

### New versions

Each time it starts, the app asks GitHub whether a newer version is out and, if so, shows a card
at the foot of the navigation. Without a connection it keeps what it knew. The request carries
only the app's name and version. Turn it off from the
version number next to the app's name, or with `rewind-os-cli update off`;
`rewind-os-cli update check` asks at once.

The MSI and the portable builds update themselves with one click ("Update", or
`rewind-os-cli update install --yes`): the app downloads the new version, installs it only when
its checksum is in the release's signed `SHA256SUMS.txt`, and opens again. The .deb, .rpm and
Flatpak are updated by the system's package manager or Flatpak.

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
rewind-os-cli dev find                       # build folders of your projects
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

### In the terminal, full screen

`rewind-os-cli tui` opens full-screen views in the terminal, for computers without a desktop or
reached over SSH: Home (drives, memory, what is worth doing), the disk scan (folders by size,
opened with Enter and left with Backspace, `f` for the largest files) and cleanup (Space picks
a category, Enter asks before cleaning). `1`–`3`, Tab or `:home`, `:disk`, `:cleanup` switch
views, Esc stops what is running, `?` lists the keys and `q` quits.

```sh
rewind-os-cli tui              # the disk view scans your home folder
sudo rewind-os-cli tui /       # the whole system, with the categories that need rights
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

`--json` prints the engine's response for scripts. `--lang es` switches the language. In a
terminal, headings, sizes, warnings and errors are coloured; output to a pipe or a file, and
`NO_COLOR=1`, have no colours.
While a scan or a cleanup runs, a line shows what it counted and the time taken. The first
Ctrl+C stops it and prints what it did so far, marked as stopped (exit code 130); the second quits.

## Development

Rules for contributors and coding agents: [AGENTS.md](AGENTS.md). Plan: [docs/ROADMAP.md](docs/ROADMAP.md).
The app icon and how to regenerate it: [assets/app-icon/README.md](assets/app-icon/README.md).

## Licence

MIT OR Apache-2.0, at your option. See [NOTICE](NOTICE).
