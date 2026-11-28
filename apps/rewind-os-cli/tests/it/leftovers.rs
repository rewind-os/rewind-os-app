//! `leftovers.find` over a made-up home: folders of things that are installed but that no
//! package manager lists (a Steam game, an audio plugin, an AppImage) are not offered, and a
//! folder nothing refers to is.

use std::error::Error;
use std::path::Path;
use std::process::Command;

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

fn dir(path: &Path) -> Result {
    Ok(std::fs::create_dir_all(path)?)
}

/// Steam's library at `steam`, with Stardew Valley installed, as Steam writes it.
#[cfg(not(windows))]
fn steam(steam: &Path) -> Result {
    let apps = steam.join("steamapps");
    dir(&apps)?;
    let library = steam.to_string_lossy().replace('\\', "\\\\");
    std::fs::write(
        apps.join("libraryfolders.vdf"),
        format!("\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{library}\"\n\t}}\n}}\n"),
    )?;
    std::fs::write(
        apps.join("appmanifest_413150.acf"),
        "\"AppState\"\n{\n\t\"appid\"\t\t\"413150\"\n\t\"name\"\t\t\"Stardew Valley\"\n\t\"installdir\"\t\t\"Stardew Valley\"\n}\n",
    )?;
    Ok(())
}

/// The names of the folders offered, run with `env` set and the XDG folders unset.
fn offered(env: &[(&str, &Path)]) -> Result<Vec<String>> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rewind-os-cli"));
    command
        .args(["--json", "run", r#"{"tool":"leftovers.find"}"#])
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_DATA_HOME");
    for (key, value) in env {
        command.env(key, value);
    }
    let out = command.output()?;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    Ok(json["data"]
        .as_array()
        .ok_or("no list of leftovers")?
        .iter()
        .filter_map(|l| l["path"].as_str())
        .filter_map(|p| Path::new(p).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .collect())
}

fn assert_only_the_abandoned_one(offered: &[String], in_use: &[&str]) {
    assert!(offered.iter().any(|n| n == "Zqxwvjk"), "{offered:?}");
    for name in in_use {
        assert!(
            !offered.iter().any(|n| n == name),
            "{name} was offered: {offered:?}"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn installed_games_plugins_and_appimages_keep_their_folders() -> Result {
    let home = tempfile::tempdir()?;
    let h = home.path();
    for name in ["StardewValley", "Toontrack", "obsidian", "Zqxwvjk"] {
        dir(&h.join(".config").join(name))?;
    }
    steam(&h.join(".local/share/Steam"))?;
    dir(&h.join(".vst3/Toontrack/EZdrummer 3.vst3"))?;
    dir(&h.join("Applications"))?;
    std::fs::write(h.join("Applications/Obsidian-1.5.3.AppImage"), b"")?;
    let offered = offered(&[("HOME", h)])?;
    assert_only_the_abandoned_one(&offered, &["StardewValley", "Toontrack", "obsidian"]);
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn installed_games_and_plugins_keep_their_folders() -> Result {
    let home = tempfile::tempdir()?;
    let h = home.path();
    let support = h.join("Library/Application Support");
    for name in ["StardewValley", "EZdrummer 3", "Zqxwvjk"] {
        dir(&support.join(name))?;
    }
    steam(&support.join("Steam"))?;
    dir(&h.join("Library/Audio/Plug-Ins/Components/EZdrummer 3.component"))?;
    let offered = offered(&[("HOME", h)])?;
    assert_only_the_abandoned_one(&offered, &["StardewValley", "EZdrummer 3"]);
    Ok(())
}

/// Program Files cannot be moved for a test (Windows sets it for every process), so Steam
/// games and plugins there are covered by the engine's tests and checked by hand; a tool on the
/// PATH is what a made-up tree can show.
#[cfg(windows)]
#[test]
fn a_tool_on_the_path_keeps_its_folder() -> Result {
    let root = tempfile::tempdir()?;
    let r = root.path();
    let local = r.join("Local");
    let roaming = r.join("Roaming");
    let data = r.join("ProgramData");
    let profile = r.join("User");
    dir(&local)?;
    dir(&profile)?;
    dir(&roaming.join("Zqxwvjk"))?;
    dir(&data.join("Qvtool").join("bin"))?;
    let mut path = std::ffi::OsString::from(data.join("Qvtool").join("bin"));
    path.push(";");
    path.push(std::env::var_os("PATH").unwrap_or_default());
    let offered = offered(&[
        ("LOCALAPPDATA", &local),
        ("APPDATA", &roaming),
        ("PROGRAMDATA", &data),
        ("USERPROFILE", &profile),
        ("PATH", Path::new(&path)),
    ])?;
    assert_only_the_abandoned_one(&offered, &["Qvtool"]);
    Ok(())
}
