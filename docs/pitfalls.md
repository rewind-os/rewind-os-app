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

## Windows: explorer.exe and installer icons

- `explorer.exe /select,<path>` parses its own command line. Rust quotes an argument with spaces
  as a whole (`"/select,C:\a b"`), which explorer does not understand, so it opens Documents.
  Pass `/select,"C:\a b"` with `raw_arg` and drop the `\\?\` prefix.
- Windows Installer saves an `<Icon>` under its `Id`: without an `.ico` extension the Start menu
  shortcut and the Settings entry show a blank icon. Small icon sizes are bitmaps, not PNG.

## Starting a program's uninstaller

`UninstallString` is a full command line in whatever shape the installer chose: quoted,
unquoted with spaces in the path, or `MsiExec.exe /I{…}` (which opens change/repair, not
remove). Do not run it through `cmd /C`: cmd strips the first and last quote when the line has
more than two, and an uninstaller that needs administrator rights fails inside cmd with error
740 while `spawn()` still reports success. `programs::launch` splits the line itself and starts
it with `Start-Process` (ShellExecute), which shows the administrator prompt and returns the
Windows error code (1223 declined, 2 missing).
