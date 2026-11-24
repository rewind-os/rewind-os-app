//! Preinstalled Store apps that can go: a closed list of Microsoft extras and sponsored apps.
//! Removal is for the signed-in person only (`Remove-AppxPackage`, never `-AllUsers` and never
//! deprovisioning), so each app can be installed again from the Microsoft Store. The Store
//! itself, the app installer, system parts, codecs and everyday tools are never in the list.

use crate::{EngineError, Result};
use rewind_os_platform::process;
use serde::{Deserialize, Serialize};

/// One app the list knows. `package` is the package name (`Microsoft.BingNews`), or a prefix
/// when it ends with `*`.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct AppSpec {
    pub package: &'static str,
    /// English; translate with `rewind_os_i18n::tr`.
    pub name: &'static str,
    /// Safe to suggest to anyone: news, tips, games and ads nobody asked for.
    pub recommended: bool,
}

const fn app(package: &'static str, name: &'static str, recommended: bool) -> AppSpec {
    AppSpec {
        package,
        name,
        recommended,
    }
}

pub const CATALOG: &[AppSpec] = &[
    app("Microsoft.BingNews", "News", true),
    app("Microsoft.BingWeather", "Weather", true),
    app("Microsoft.BingFinance", "Money", true),
    app("Microsoft.BingSports", "Sports", true),
    app("Microsoft.GetHelp", "Get Help", true),
    app("Microsoft.Getstarted", "Tips", true),
    app("Microsoft.WindowsFeedbackHub", "Feedback Hub", true),
    app(
        "Microsoft.MicrosoftSolitaireCollection",
        "Solitaire Collection",
        true,
    ),
    app(
        "Microsoft.MicrosoftOfficeHub",
        "Microsoft 365 (Office) hub",
        true,
    ),
    app("Microsoft.PowerAutomateDesktop", "Power Automate", true),
    app("Microsoft.549981C3F5F10", "Cortana", true),
    app(
        "Microsoft.MixedReality.Portal",
        "Mixed Reality Portal",
        true,
    ),
    app("Microsoft.WindowsMaps", "Maps", true),
    app("Microsoft.ZuneVideo", "Movies & TV", true),
    app("Microsoft.People", "People", true),
    app("Microsoft.Windows.DevHome", "Dev Home", true),
    app(
        "MicrosoftCorporationII.MicrosoftFamily",
        "Microsoft Family",
        true,
    ),
    app("MicrosoftCorporationII.Windows365", "Windows 365", true),
    app("Clipchamp.Clipchamp", "Clipchamp", true),
    app("king.com.*", "Candy Crush and other King games", true),
    app("Disney.*", "Disney+", true),
    app("BytedancePte.Ltd.TikTok", "TikTok", true),
    app("Facebook.*", "Facebook and Instagram", true),
    app("AmazonVideo.PrimeVideo", "Prime Video", true),
    // Useful to some people: offered, never suggested.
    app(
        "Microsoft.BingSearch",
        "Bing Search (web results in Start)",
        false,
    ),
    app("Microsoft.Todos", "Microsoft To Do", false),
    app("Microsoft.OutlookForWindows", "Outlook (new)", false),
    app("Microsoft.YourPhone", "Phone Link", false),
    app("Microsoft.GamingApp", "Xbox", false),
    app("Microsoft.SkypeApp", "Skype", false),
    app("SpotifyAB.SpotifyMusic", "Spotify", false),
];

/// The catalog entry for a package name.
pub fn spec(package_name: &str) -> Option<&'static AppSpec> {
    CATALOG.iter().find(|s| match s.package.strip_suffix('*') {
        Some(prefix) => package_name.starts_with(prefix),
        None => package_name.eq_ignore_ascii_case(s.package),
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct StoreApp {
    /// Package name (`Microsoft.BingNews`), used by `apps.remove`.
    pub id: String,
    pub name: &'static str,
    pub version: String,
    pub recommended: bool,
}

#[derive(Debug, Deserialize)]
struct RawApp {
    name: String,
    full: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    removable: bool,
}

const LIST_SCRIPT: &str = r#"[Console]::OutputEncoding = [Text.Encoding]::UTF8
ConvertTo-Json -Compress -InputObject @(Get-AppxPackage | Where-Object { -not $_.IsFramework } | ForEach-Object {
  [pscustomobject]@{ name = $_.Name; full = $_.PackageFullName; version = [string]$_.Version; removable = -not $_.NonRemovable }
})"#;

fn list_output() -> Option<String> {
    let done = process::run(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", LIST_SCRIPT],
    )
    .ok()?;
    done.success().then_some(done.stdout)
}

fn raw_apps() -> Vec<RawApp> {
    list_output().map(|out| parse_raw(&out)).unwrap_or_default()
}

fn parse_raw(json: &str) -> Vec<RawApp> {
    serde_json::from_str(json.trim().trim_start_matches('\u{feff}')).unwrap_or_default()
}

/// The installed apps of the catalog, from `Get-AppxPackage`'s output as the list script
/// prints it. Apps Windows marks as not removable are left out.
pub fn parse(json: &str) -> Vec<StoreApp> {
    let mut apps: Vec<StoreApp> = parse_raw(json)
        .into_iter()
        .filter(|a| a.removable)
        .filter_map(|a| {
            let spec = spec(&a.name)?;
            Some(StoreApp {
                id: a.name,
                name: spec.name,
                version: a.version,
                recommended: spec.recommended,
            })
        })
        .collect();
    apps.sort_by(|a, b| b.recommended.cmp(&a.recommended).then(a.name.cmp(b.name)));
    apps.dedup_by(|a, b| a.id == b.id);
    apps
}

pub fn list() -> Vec<StoreApp> {
    list_output().map(|out| parse(&out)).unwrap_or_default()
}

#[derive(Debug, Clone, Serialize)]
pub struct Removal {
    pub id: String,
    pub error: Option<String>,
}

/// Package full names are letters, digits and `._-~`; anything else is refused before it can
/// reach a command line.
fn safe_full_name(full: &str) -> bool {
    !full.is_empty()
        && full
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '~'))
}

/// Removes the apps with these package names for the signed-in person. Names outside the
/// catalog are refused.
pub fn remove(ids: &[String]) -> Result<Vec<Removal>> {
    if let Some(unknown) = ids.iter().find(|id| spec(id).is_none()) {
        return Err(EngineError::NotFound(unknown.clone()));
    }
    let installed = raw_apps();
    Ok(ids
        .iter()
        .map(|id| {
            let error = match installed.iter().find(|a| a.name == *id && a.removable) {
                None => Some("not installed".to_owned()),
                Some(app) if !safe_full_name(&app.full) => {
                    Some("unexpected package name".to_owned())
                }
                Some(app) => {
                    let script = format!(
                        "$ErrorActionPreference = 'Stop'; Remove-AppxPackage -Package '{}'",
                        app.full
                    );
                    match process::run(
                        "powershell",
                        &["-NoProfile", "-NonInteractive", "-Command", &script],
                    ) {
                        Ok(d) if d.success() => None,
                        Ok(d) => Some(
                            d.stderr
                                .lines()
                                .next()
                                .unwrap_or("failed")
                                .trim()
                                .to_owned(),
                        ),
                        Err(e) => Some(e.to_string()),
                    }
                }
            };
            Removal {
                id: id.clone(),
                error,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn essentials_are_never_in_the_catalog() {
        for keep in [
            "Microsoft.WindowsStore",
            "Microsoft.StorePurchaseApp",
            "Microsoft.DesktopAppInstaller",
            "Microsoft.SecHealthUI",
            "Microsoft.WindowsCalculator",
            "Microsoft.WindowsNotepad",
            "Microsoft.WindowsTerminal",
            "Microsoft.Windows.Photos",
            "Microsoft.Paint",
            "Microsoft.HEVCVideoExtension",
            "Microsoft.Xbox.TCUI",
            "Microsoft.XboxIdentityProvider",
            "Microsoft.MicrosoftEdge.Stable",
            "MicrosoftWindows.Client.WebExperience",
        ] {
            assert!(spec(keep).is_none(), "{keep} must never be offered");
        }
    }

    #[test]
    fn only_installed_removable_catalog_apps_are_listed() {
        let json = r#"[
          {"name":"Microsoft.BingNews","full":"Microsoft.BingNews_4.55.62231.0_x64__8wekyb3d8bbwe","version":"4.55","removable":true},
          {"name":"Microsoft.WindowsStore","full":"Microsoft.WindowsStore_22410_x64__8wekyb3d8bbwe","version":"22410","removable":true},
          {"name":"Microsoft.Todos","full":"Microsoft.Todos_2.1_x64__8wekyb3d8bbwe","version":"2.1","removable":true},
          {"name":"king.com.CandyCrushSaga","full":"king.com.CandyCrushSaga_1.0_x86__kgqvnymyfvs32","version":"1.0","removable":false}
        ]"#;
        let apps = parse(json);
        let ids: Vec<&str> = apps.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["Microsoft.BingNews", "Microsoft.Todos"]);
        assert!(apps[0].recommended && !apps[1].recommended);
    }

    #[test]
    fn package_names_that_could_break_a_command_are_refused() {
        assert!(safe_full_name(
            "Microsoft.BingNews_4.55.62231.0_x64__8wekyb3d8bbwe"
        ));
        assert!(!safe_full_name("x'; Remove-Item C:\\ -Recurse; '"));
        assert!(!safe_full_name(""));
    }

    #[test]
    fn names_outside_the_catalog_are_refused_before_anything_runs() {
        assert!(remove(&["Microsoft.WindowsStore".to_owned()]).is_err());
    }
}
