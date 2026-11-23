# Safety

Rewind OS must never leave a computer unable to start or wipe someone's files. These are the
guarantees, where they are enforced, and the tests that hold them.

## What can change the computer, and how it is limited

| Action | Limit |
|---|---|
| Junk cleanup | Only curated categories of caches and temporary files, each a fixed path template. Every matched path passes the guard's cleanup check. In system locations only files are deleted and every folder is kept. Files in use are skipped. Categories that need rights are skipped without them. |
| Move to recycle bin (analyzer, leftovers) | Passes the guard's removal check. Goes to the recycle bin, so it can be restored. |
| Startup programs | Only the system's own on/off records (Windows "startup approved", XDG `Hidden=` in the user's folder). Nothing is deleted. Essential entries (security, sign-in, accessibility, sound, the desktop session) cannot be turned off. |
| Maintenance | Only the system's own tools (DISM component cleanup, `powercfg /hibernate`, `journalctl --vacuum-time`), which their vendors support. |
| Uninstall | Starts the program's own uninstaller; Rewind OS removes nothing itself. |

Everything that deletes or changes something asks first in the app and needs `--yes` in the
CLI.

## The guard (`crates/engine/src/safety.rs`)

Every removal goes through it in the engine, whatever front end asked:

- **Never removed, and nothing containing them**: drive roots, the users folder, every user
  profile, the home folder, Program Files, ProgramData, AppData, the personal folders
  (Documents, Desktop, Pictures, Music, Videos, OneDrive), `/`, `/home`, `/Users`,
  `/Applications`, `~/Library`.
- **Never removed by hand**: anything inside the operating system (`C:\Windows`, `C:\Boot`,
  `C:\Recovery`, `/usr`, `/etc`, `/var`, `/System`, `/Library`…), and `pagefile.sys`,
  `hiberfil.sys`, `bootmgr`.
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
- `crates/engine/tests/it/startup.rs`: essential entries cannot be turned off; ids cannot escape
  the autostart folders.
- `crates/engine/src/walk.rs`: links are not followed; system locations keep their folders.

## Rules for changes

A new cleanup category, tool or removal path MUST go through the guard, MUST NOT widen it
without a test that shows why the new place is safe, and SHOULD prefer the recycle bin to
deletion for anything that is not a regenerable cache.
