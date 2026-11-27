# Safety

Rewind OS must never leave a computer unable to start or wipe someone's files. These are the
guarantees, where they are enforced, and the tests that hold them.

## What can change the computer, and how it is limited

| Action | Limit |
|---|---|
| Junk cleanup | Only curated categories of caches and temporary files, each a fixed path template. Every matched path passes the guard's cleanup check. In system locations only files are deleted and every folder is kept. Files in use are skipped. Categories that need rights are skipped without them. |
| Move to recycle bin (analyzer, leftovers, duplicates) | Passes the guard's removal check. Goes to the recycle bin, so it can be restored. |
| Empty folders (`files.trash_empty`) | Only folders a disk scan found with no files below them and that are, checked on the disk without following links, empty or holding only empty folders (`empty::is_empty_folder`); links and second names of hard-linked files count as content. Hidden folders, folders holding a hidden, program or project folder, and the folders `Guard::is_program_folder` names are never offered, and the guard's removal check applies. Each is checked again right before it goes and left alone if something was added. Goes to the recycle bin. Over MCP, only inside `--automation-write-root`. |
| Duplicate files | Only files with identical contents, compared in full. Hard links to one file are not offered, since removing one frees nothing. The operating system's folders are not searched, and neither are the folders of programs and projects (`Guard::is_program_folder`: Program Files, ProgramData, AppData, `~/.config`, `~/.local/share`, `~/.cache`, `/opt`, `/Applications`, `~/Library`, and any `.git`, `node_modules`, `site-packages`, virtual environment, `steamapps` or macOS bundle), where every copy is needed where it is; the skipped folders are reported. The app never lets the last copy of a file be chosen (`duplicates::keeps_a_copy`, checked again when the dialog is confirmed). |
| Build folders (`dev.clean`) | Only folders `projects::identify` recognises by the file beside or inside them (`node_modules` with `package.json` and a lockfile, `target` with `Cargo.toml` and Cargo's `CACHEDIR.TAG` or with `pom.xml`, a virtual environment's `pyvenv.cfg` beside a Python project file, and so on). Never inside a hidden folder, the apps' data (AppData, `~/Library`, XDG folders), a program's folder or the system: checked for every folder above it again at removal, so a path typed by hand gets no further than the search. Then the guard's removal check. Deleted for good, since each project's tool makes them again; links inside are never followed. Python environments are marked "check first" and left out of "select unused". Over MCP, only inside `--automation-write-root`. |
| Make Windows lighter | Only the settings in a closed catalog (`crates/engine/src/tweaks.rs`): documented registry values and policies. Each records exactly how it goes back to Windows' default, and a setting that fails halfway puts back the values it already wrote. Nothing is uninstalled. |
| Preinstalled Store apps | Only a closed list of Microsoft extras and sponsored apps (`crates/engine/src/apps.rs`); the Store, the app installer, security, codecs, Xbox components and everyday tools are never in it. Removed for the signed-in person only (`Remove-AppxPackage`, never `-AllUsers` or deprovisioning), so each can be installed again from the Store. Asks first; `--yes` in the CLI. |
| Startup programs | Only the system's own on/off records (Windows "startup approved", XDG `Hidden=` in the user's folder, `schtasks /Change /DISABLE` for scheduled tasks, `launchctl disable gui/<uid>/<label>` for macOS LaunchAgents, whose files are never changed; Apple's `com.apple.*` agents are never listed and are refused if asked for). Nothing is deleted. macOS login items have no off switch: turning one off writes it to `~/Library/Application Support/rewind-os/login-items-off.json` first and only then takes it off System Events' list (never touching the app), so it can be put back as it was; if taking it off fails, the record is undone. Only items on the list can be taken off, and one is put back only from that record and while its app is still there. Windows' own scheduled tasks (`\Microsoft\…`) are never listed and are refused if asked for. Services: only those programs installed (never drivers, anything in the Windows folder, `svchost` services or Windows Defender); turning one off sets it to start when needed, never to disabled, and the app remembers which it changed (`%ProgramData%\rewind-os`) so only those can be set back. Security, VPN, sound, input and backup services are never turned off. Essential entries (security, sign-in, accessibility, sound, the desktop session) cannot be turned off. |
| Maintenance | Only the system's own tools (DISM component cleanup, `powercfg /hibernate`, `journalctl --vacuum-time`), which their vendors support. |
| Restore points (`restore.clean`) | Windows only, as administrator. Deletes the shadow copies `Win32_ShadowCopy` lists for each drive except the newest one (`restore::all_but_newest`, tested), as Windows' own Disk Cleanup does, so the latest restore point always remains. Each copy is named by its GUID, checked (`restore::is_shadow_id`) before it reaches the PowerShell command that removes it; copies Windows refuses to delete are reported. Cannot be undone: asks first; `--yes` in the CLI. |
| Old driver packages (`drivers.clean`) | Windows only, as administrator. Packages are read from Windows' own record (`DriverDatabase\DriverPackages`) and grouped by driver: the same original INF, provider and device class. The newest of each group and every package a device uses (`Win32_PnPSignedDriver`) stay (`drivers::old_packages`, tested); when Windows does not say which are in use, nothing is offered. Only packages Windows added (`oem<n>.inf`, `safety::is_oem_driver_name`, tested) reach `pnputil /delete-driver`, never with `/force`, so Windows itself refuses a package a device needs. Each name is checked again against a fresh list just before. Cannot be undone: asks first; `--yes` in the CLI. |
| Registry leftovers | Report only. Rewind OS lists vendor keys under `Software` that nothing installed refers to, and never removes a registry key. |
| MCP server (`rewind-os-cli mcp`) | Offers only tools that change nothing by default. Paths are read only inside `--automation-read-root`, moved to the recycle bin only inside `--automation-write-root` (resolved, so `..` and links cannot leave it), and still pass the guard. Other changes need `--allow-system-changes`. Restarting as administrator is never offered. |
| Administrator rights on Linux and macOS (`system.run_elevated`) | The app never runs as root. One request at a time runs through `rewind-os-cli` after the system's password prompt (`pkexec`, the macOS authorization dialog), and only system cleanup and journal maintenance may run that way, so root never works on root's own home folder. Never offered over MCP. |
| Updates (`update.install`) | Only a version GitHub lists as the latest published release, from this project's release downloads, over https. Its `SHA256SUMS.txt` must carry a minisign signature by the key built into the app (`packaging/minisign.pub`) whose trusted comment names that version, and the package must match its checksum there; otherwise nothing is installed. The MSI goes to the Windows Installer, which asks for permission. A portable copy (a folder unpacked from the zip or tar.gz) has only its two programs replaced: the running ones are renamed `.old` first and put back if any copy fails. Copies installed by a package manager or Flatpak, and macOS, are never changed. The next start removes the `.old` programs and the download, through the guard. Asks first; `--yes` in the CLI. |
| Uninstall | Windows: starts the program's own uninstaller; Rewind OS removes nothing itself. Linux: only programs the page lists (never libraries or the base system), through the package manager. APT is simulated first and refused if anything besides the package would go; pacman refuses by itself when others depend on it; Flatpak and Snap remove the app only. RPM uses `rpm -e`, which removes one package and is checked first with `--test`, failing when anything depends on it. `remove`, not `purge`: settings stay. |

Everything that deletes or changes something asks first in the app and needs `--yes` in the
CLI.

## The guard (`crates/engine/src/safety.rs`)

Every removal goes through it in the engine, whatever front end asked:

- **Never removed, and nothing containing them**: drive roots, the users folder, every user
  profile, the home folder, Program Files, ProgramData, AppData, the personal folders
  (Documents, Desktop, Pictures, Music, Videos, OneDrive), `/`, `/home`, `/Users`,
  `/Applications`, `~/Library`.
- **Never removed by hand**: anything inside the operating system (`C:\Windows`, `C:\Boot`,
  `C:\Recovery`, `/usr`, `/etc`, `/var`, `/nix`, `/System`, `/Library`…), and `pagefile.sys`,
  `hiberfil.sys`, `bootmgr`. On Windows also: the boot and system folders at the root of
  *any* drive (a second Windows on `D:`), the parts of Windows in Program Files (`Windows NT`,
  `WindowsPowerShell`, `dotnet`…), `ProgramData\Microsoft`, `ProgramData\Package Cache`
  (installers programs need to repair or uninstall themselves), and in every profile the
  registry hive (`NTUSER.DAT`, `UsrClass.dat`), `AppData\Local\Packages` and Windows' own
  `AppData\…\Microsoft\Windows` folders.
- **Never removed, by hand or by cleanup**: in every profile, keys and saved passwords
  (`AppData\…\Microsoft\Protect`, `Crypto`, `Credentials`, `Vault`, `SystemCertificates`;
  `~/.ssh`, `~/.gnupg`, `~/.pki`, `~/.password-store`, `~/.local/share/keyrings`,
  `~/Library/Keychains`).
- **Never touched by cleanup**: `System32`, `SysWOW64`, `WinSxS`, `servicing`, fonts, drivers,
  the installer cache, Windows Defender, `/etc`, `/boot`, `/usr`, the package databases, and the
  personal folders.

Paths are compared after resolving links and `..`, ignoring case where the file system does, so
a junction into `System32` or `C:\Windows\..\WINDOWS\System32` is refused too. A bad
environment variable (`TEMP=C:\Users`) cannot widen a cleanup: its targets would contain user
profiles and are refused.

Measuring and deleting never follow symbolic links or junctions.

## Tests

- `crates/engine/tests/it/safety.rs`: system paths, profiles and personal folders are refused;
  ordinary files and the cleanup locations are allowed (Unix paths on Linux and macOS, Windows
  paths on Windows, each in CI).
- `crates/engine/tests/it/cleanup.rs`: a cleanup deletes inside its folder only.
- `crates/engine/tests/it/tweaks.rs`: every setting applies and undoes back to the default; a
  failure halfway leaves nothing changed.
- `crates/engine/tests/it/startup.rs`: essential entries cannot be turned off; ids cannot escape
  the autostart folders.
- `crates/engine/src/walk.rs`: links are not followed; system locations keep their folders.

## Rules for changes

A new cleanup category, tool or removal path MUST go through the guard, MUST NOT widen it
without a test that shows why the new place is safe, and SHOULD prefer the recycle bin to
deletion for anything that is not a regenerable cache.
