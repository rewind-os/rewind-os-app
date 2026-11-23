# Pitfalls

Things that look wrong but are not, and mistakes worth not repeating.

- **WinSxS looks huge.** Most files in `C:\Windows\WinSxS` are hard links shared with
  `C:\Windows\System32`. The analyzer counts each link, so the folder looks several GB larger
  than it is. Use Maintenance → "Measure real size" (`dism /AnalyzeComponentStore`). Never delete
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
