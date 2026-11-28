//! The scheduled cleanup on Linux, against a made-up home and a stand-in `systemctl`: turning it
//! on writes the user's timer, a run cleans only the scheduled caches and keeps a record, and
//! turning it off removes the timer.

#![cfg(target_os = "linux")]

use std::error::Error;
use std::path::Path;
use std::process::{Command, Output};

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

fn cli(home: &Path, bin: &Path, args: &[&str]) -> Result<Output> {
    let mut path = std::ffi::OsString::from(bin);
    path.push(":");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    Ok(Command::new(env!("CARGO_BIN_EXE_rewind-os-cli"))
        .args(args)
        .env("HOME", home)
        .env("PATH", path)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_CACHE_HOME")
        .output()?)
}

#[test]
fn on_run_and_off_touch_only_the_schedule_and_the_caches() -> Result {
    let root = tempfile::tempdir()?;
    let home = root.path().join("home");
    let bin = root.path().join("bin");
    std::fs::create_dir_all(&bin)?;
    // Says the timer is enabled while its file is there.
    let systemctl = bin.join("systemctl");
    std::fs::write(
        &systemctl,
        "#!/bin/sh\ncase \"$*\" in *is-enabled*) [ -f \"$HOME/.config/systemd/user/rewind-os-cleanup.timer\" ] && echo enabled || echo disabled;; esac\nexit 0\n",
    )?;
    let mut permissions = std::fs::metadata(&systemctl)?.permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    std::fs::set_permissions(&systemctl, permissions)?;
    let cache = home.join(".cache/thumbnails/normal");
    std::fs::create_dir_all(&cache)?;
    std::fs::write(cache.join("a.png"), vec![0u8; 4096])?;
    let documents = home.join("Documents");
    std::fs::create_dir_all(&documents)?;
    std::fs::write(documents.join("notes.txt"), b"mine")?;

    // Without --yes nothing changes.
    let refused = cli(&home, &bin, &["schedule", "on"])?;
    assert!(!refused.status.success());
    let units = home.join(".config/systemd/user");
    assert!(!units.join("rewind-os-cleanup.timer").exists());

    assert!(
        cli(&home, &bin, &["schedule", "on", "--yes"])?
            .status
            .success()
    );
    let timer = std::fs::read_to_string(units.join("rewind-os-cleanup.timer"))?;
    assert!(timer.contains("Persistent=true"));

    assert!(
        cli(&home, &bin, &["schedule", "run", "--yes"])?
            .status
            .success()
    );
    assert!(!cache.join("a.png").exists(), "the cache stayed");
    assert!(
        documents.join("notes.txt").exists(),
        "a document was removed"
    );
    let record =
        std::fs::read_to_string(home.join(".local/share/rewind-os/scheduled-cleanups.json"))?;
    assert!(record.contains("\"files\": 1"), "{record}");

    assert!(cli(&home, &bin, &["schedule", "off"])?.status.success());
    assert!(!units.join("rewind-os-cleanup.timer").exists());
    assert!(!units.join("rewind-os-cleanup.service").exists());
    Ok(())
}
