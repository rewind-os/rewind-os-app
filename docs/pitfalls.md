# Pitfalls

Things that look wrong but are not, and mistakes worth not repeating.

- **WinSxS looks huge.** Most files in `C:\Windows\WinSxS` are hard links shared with
  `C:\Windows\System32`. The analyzer counts each hard-linked file once (inside the Windows
  folder on Windows, where asking costs opening the file; everywhere on Linux and macOS), so the
  total is right, but the folder found first gets the size: WinSxS or System32 can look larger
  than the other depending on the scan order. The component store also shares files in other
  ways (compression, deltas), so its real size still comes from DISM. Use Maintenance → "Measure real size" (`dism /AnalyzeComponentStore`). Never delete
  files from WinSxS by hand.
- **Moving to the recycle bin frees nothing yet.** Space comes back when the bin is emptied.
  Every message that moves files says so.
- **Junctions and symbolic links.** `C:\Users\<you>\Application Data` and similar are junctions
  back into the profile. `std::fs::FileType::is_dir()` is false for them on Windows, and
  `walk.rs` checks `symlink_metadata`, so neither scanning nor deleting follows them. Keep it so.
- **Files in use.** Browsers and Windows keep cache files open. Deleting them fails; that is
  expected and reported as "files in use were kept", never as an error of the whole category.
- **Admin-only folders.** Without elevation `C:\Windows\Temp` and friends are unreadable. Those
  categories are shown as "needs administrator" instead of a misleading 0 B.
- **`explorer.exe /select,` returns exit code 1** even when it works; only a failure to start it
  counts as an error.
- **sysinfo 0.40** returns `Result` from `System::new()` and friends; each part of the overview
  degrades on its own instead of failing the page.
- **egui 0.37** renamed `SidePanel`/`TopBottomPanel` to `Panel`, `App::update` to `App::ui`,
  `Id::new` to `Id::unique` and screenshots use a `ScreenshotCallback`.
- **Rust 2024 makes `std::env::set_var` unsafe**, and `unsafe` is forbidden here. Code that
  reads folders from the environment takes them as a parameter instead (`cleanup::Places`,
  `startup::xdg::Dirs`), so tests pass temporary folders rather than changing the environment.
- **Glob matching is case-insensitive only on Windows.** Linux and macOS cache folders differ
  only by case sometimes (`Code Cache` vs `code cache`); keep templates exact.
- **XDG autostart overrides.** A file in `~/.config/autostart` with the same name as one in
  `/etc/xdg/autostart` replaces it. Turning a system entry off writes the user copy with
  `Hidden=true`; never edit `/etc`. Entry ids are file names and are checked, so an id can never
  name a path outside the autostart folders.
- **macOS Trash and Full Disk Access.** Listing `~/.Trash` can fail without Full Disk Access;
  the recycle bin category then shows the error instead of 0 B.
- **Cloud placeholders.** OneDrive "files on demand" look like normal files, but reading one
  downloads it. Anything that reads contents (the duplicate finder) checks
  `platform::files::is_cloud_placeholder` first and reports what it skipped.

## Reading the master file table

- `\\.\C:` opened for reading needs administrator rights, and Windows refuses reads that do
  not start and end on a sector: `mft::read` reads whole clusters only. The table is parsed by
  our own code, with every offset checked, because a parser that panics on a damaged record
  would abort the app (release builds abort on panic).
- A `subst` drive opens the volume of the folder it stands for. The scan checks that the files
  and folders at the top of the scanned folder are the ones the table names, and walks the
  folders otherwise.
- The table also measures what nobody may list (`System Volume Information`, other people's
  profiles), so a drive can look larger than a folder-by-folder scan of it. NTFS's own files
  (`$MFT`, `$LogFile`, `$Extend`) are left out. Symbolic links and junctions are left out too,
  as everywhere; cloud files are kept, with the space they really take.
- What Windows has not written to disk yet is not in the table: a file saved a moment ago can
  be missing until the next scan.

## Windows: explorer.exe and installer icons

- `explorer.exe /select,<path>` parses its own command line. Rust quotes an argument with spaces
  as a whole (`"/select,C:\a b"`), which explorer does not understand, so it opens Documents.
  Pass `/select,"C:\a b"` with `raw_arg` and drop the `\\?\` prefix. It also opens Documents
  for a path that no longer exists, so `shell::reveal` opens the nearest existing folder then.
- Windows Installer saves an `<Icon>` under its `Id`: without an `.ico` extension the Start menu
  shortcut and the Settings entry show a blank icon. Small icon sizes are bitmaps, not PNG.

## Windows: the driver database

- The complete record of driver packages is `HKLM\DRIVERS\DriverDatabase\DriverPackages`, a
  hive Windows loads on its own. `HKLM\SYSTEM\DriverDatabase\DriverPackages` looks the same but
  holds only some packages: on one computer it missed every NVIDIA and Intel Bluetooth package,
  so nine versions of one driver looked like none. Compare with `pnputil /enum-drivers`.
- `HKLM\DRIVERS` is loaded only while something uses the driver store and unloaded about two
  minutes later; then opening it fails as if it did not exist. Listing the store
  (`pnputil /enum-drivers`, no rights needed) loads it again.

## Starting a program's uninstaller

`UninstallString` is a full command line in whatever shape the installer chose: quoted,
unquoted with spaces in the path, or `MsiExec.exe /I{…}` (which opens change/repair, not
remove). Do not run it through `cmd /C`: cmd strips the first and last quote when the line has
more than two, and an uninstaller that needs administrator rights fails inside cmd with error
740 while `spawn()` still reports success. `programs::launch` splits the line itself and starts
it with `Start-Process` (ShellExecute), which shows the administrator prompt and returns the
Windows error code (1223 declined, 2 missing).
- **A Linux release that warns about `GLIBC_2.39` on Debian 12.** Rust's standard library asks
  for the newest glibc functions of the system it is built on (`pidfd_spawnp`, as weak symbols:
  the program still runs, but the loader warns). Releases are built on Ubuntu 22.04 (glibc 2.35)
  and `packaging/linux/package.sh` stops when the binaries need more than `REWIND_OS_MAX_GLIBC`.
  Do not move the Linux release jobs to a newer runner without raising that floor on purpose.
  The release caches are keyed by runner: moving to 22.04 first failed because the arm64 job
  restored a 24.04 cache whose proc macros (`zvariant_derive`) needed glibc 2.39 to load.
