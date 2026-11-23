# Roadmap

Ids are `R<milestone>.<n>`. `- [ ]` is pending, `- [x]` is done.

## R0 — First version

- [x] R0.1 Engine with one `Request` per tool and a CLI that reaches every tool
- [x] R0.2 Overview: drives (SSD/HDD), memory, `hiberfil.sys` / `pagefile.sys` / `swapfile.sys`
- [x] R0.3 Disk analyzer: parallel scan, folder tree, treemap, largest files, size per extension
- [x] R0.4 Junk cleanup by category, with minimum age, admin detection and honest reports
- [x] R0.5 Program leftovers: folders nothing on the system refers to, moved to the recycle bin
- [x] R0.6 Installed programs with search, sort and their own uninstaller
- [x] R0.7 Startup programs on/off using Windows' own "startup approved" records
- [x] R0.8 Maintenance: component store analysis and cleanup, hibernation on/off
- [x] R0.9 Spanish translation; light and dark themes following the system

## R1 — Next

- [ ] R1.1 Verify every feature on a real Windows 11 machine (only cross-checked so far)
- [ ] R1.2 Duplicate files finder (size, then partial hash, then full hash)
- [ ] R1.3 Size on disk (compressed and sparse files) next to logical size
- [ ] R1.4 Count hard links once, so WinSxS shows its real size in the analyzer
- [ ] R1.5 Leftover registry keys of uninstalled programs (report only, export before removing)
- [ ] R1.6 Scheduled tasks and services in the startup page
- [ ] R1.7 MCP server (`rewindows-cli mcp`) with `--automation-read-root` / `--automation-write-root`
- [ ] R1.8 Live-region announcements when a background job finishes
- [x] R1.9 GitHub Actions: CI on Linux and Windows, release workflow with portable zips (x64, arm64)
- [ ] R1.11 MSI installer (WiX) next to the portable zip
- [ ] R1.12 Code signing certificate (Azure Trusted Signing or a .pfx) so SmartScreen does not warn
- [ ] R1.10 Faster scans via the NTFS master file table when elevated
