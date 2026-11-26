//! The power plan: how much the system favours speed over saving energy.
//!
//! - Windows: the plans `powercfg /list` shows, switched with `powercfg /setactive` (no
//!   administrator rights needed). Computers with modern standby only have "Balanced"; there
//!   the power mode in Settings (best efficiency, balanced, best performance) is what counts.
//!   Windows has no documented way to change that mode from outside, so it is read from the
//!   registry and the Settings page is opened to change it.
//! - Linux: power-profiles-daemon (or tuned-ppd), through `powerprofilesctl`.
//!
//! Switching is undone by switching back.

use crate::{EngineError, Os, Result};
use rewind_os_platform::process;
use rewind_os_platform::registry::{self, Hive};
use serde::Serialize;

/// What a plan is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Saver,
    Balanced,
    Performance,
    /// Windows' "Ultimate performance".
    Ultimate,
    /// A plan a manufacturer or the user made.
    Custom,
}

impl Kind {
    /// English; translate with `rewind_os_i18n::tr`.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Saver => "Power saver",
            Kind::Balanced => "Balanced",
            Kind::Performance => "High performance",
            Kind::Ultimate => "Ultimate performance",
            Kind::Custom => "Custom",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    /// The GUID on Windows, the profile name on Linux; what `power.set` takes.
    pub id: String,
    /// The name the system gives a custom plan; known plans are named by `kind`.
    pub name: String,
    pub kind: Kind,
    pub active: bool,
    /// Why the system holds this plan back right now (Linux: "lap-detected", "high-operating-
    /// temperature").
    pub degraded: Option<String>,
}

/// Windows' power mode on computers with modern standby.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    BestEfficiency,
    Balanced,
    BetterPerformance,
    BestPerformance,
}

impl Mode {
    /// English; translate with `rewind_os_i18n::tr`.
    pub fn label(self) -> &'static str {
        match self {
            Mode::BestEfficiency => "Best power efficiency",
            Mode::Balanced => "Balanced",
            Mode::BetterPerformance => "Better performance",
            Mode::BestPerformance => "Best performance",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub plans: Vec<Plan>,
    /// Windows' power mode when plugged in and on battery, when it has one.
    pub mode_plugged_in: Option<Mode>,
    pub mode_on_battery: Option<Mode>,
}

impl Report {
    pub fn active(&self) -> Option<&Plan> {
        self.plans.iter().find(|p| p.active)
    }
}

/// English reason, translated with `rewind_os_i18n::tr`.
pub const NO_PROFILES: &str =
    "Install power-profiles-daemon so Rewind OS can switch power profiles.";
pub const UNKNOWN_PLAN: &str = "This power plan is not on this computer.";

const BALANCED: &str = "381b4222-f694-41f0-9685-ff5bb260df2e";
const HIGH: &str = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";
const SAVER: &str = "a1841308-3540-4fab-bc81-f71973a5c3f8";
const ULTIMATE: &str = "e9a42b02-d5df-448d-aa00-03f14749eb61";

pub fn report() -> Result<Report> {
    match Os::current() {
        Os::Windows => windows(),
        Os::Linux => linux(),
        Os::MacOs => Err(EngineError::Unsupported {
            tool: "power.list",
            os: Os::MacOs.name(),
        }),
    }
}

/// Makes plan `id` (as [`report`] lists it) the active one.
pub fn set(id: &str) -> Result<()> {
    // Only a plan the system lists is passed on, so nothing else reaches the command line.
    let known = report()?.plans.into_iter().any(|p| p.id == id);
    if !known {
        return Err(EngineError::Refused(UNKNOWN_PLAN));
    }
    match Os::current() {
        Os::Windows => process::run_checked("powercfg.exe", &["/setactive", id])?,
        Os::Linux => process::run_checked("powerprofilesctl", &["set", id])?,
        Os::MacOs => {
            return Err(EngineError::Unsupported {
                tool: "power.set",
                os: Os::MacOs.name(),
            });
        }
    };
    Ok(())
}

/// Opens the system's own power settings (Windows' power mode lives there).
pub fn open_settings() -> Result<()> {
    Ok(rewind_os_platform::shell::open_settings("powersleep")?)
}

// ---- Windows ----------------------------------------------------------------------------------

fn windows() -> Result<Report> {
    let out = process::run_checked("powercfg.exe", &["/list"])?;
    const KEY: &str = r"SYSTEM\CurrentControlSet\Control\Power\User\PowerSchemes";
    let mode = |name: &str| {
        registry::value(Hive::LocalMachine, KEY, name)
            .and_then(|v| v.as_text().and_then(windows_mode))
    };
    Ok(Report {
        plans: parse_powercfg(&out.stdout),
        mode_plugged_in: mode("ActiveOverlayAcPowerScheme"),
        mode_on_battery: mode("ActiveOverlayDcPowerScheme"),
    })
}

/// The power mode a registry GUID stands for.
pub fn windows_mode(guid: &str) -> Option<Mode> {
    match guid.trim().to_ascii_lowercase().as_str() {
        "961cc777-2547-4f9d-8174-7d86181b8a7a" => Some(Mode::BestEfficiency),
        "00000000-0000-0000-0000-000000000000" | "" => Some(Mode::Balanced),
        "3af9b8d9-7c97-431d-ad78-34a8bfea439f" => Some(Mode::BetterPerformance),
        "ded574b5-45a0-4f42-8737-46345c09c238" => Some(Mode::BestPerformance),
        _ => None,
    }
}

fn is_guid(text: &str) -> bool {
    text.len() == 36
        && text.chars().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// `powercfg /list`, in any language: each line with a GUID is a plan, its name in brackets
/// and a `*` after the active one.
pub fn parse_powercfg(output: &str) -> Vec<Plan> {
    let plans = output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let guid = line
                .split_whitespace()
                .find(|w| is_guid(w))?
                .to_ascii_lowercase();
            let name = line
                .split_once('(')
                .and_then(|(_, rest)| rest.rsplit_once(')'))
                .map(|(name, _)| name.trim().to_owned())
                .unwrap_or_default();
            let kind = match guid.as_str() {
                BALANCED => Kind::Balanced,
                HIGH => Kind::Performance,
                SAVER => Kind::Saver,
                ULTIMATE => Kind::Ultimate,
                _ => Kind::Custom,
            };
            Some(Plan {
                active: line.ends_with('*'),
                id: guid,
                name,
                kind,
                degraded: None,
            })
        })
        .collect();
    in_order(plans)
}

/// Saver, balanced, performance: the order people read them in.
fn in_order(mut plans: Vec<Plan>) -> Vec<Plan> {
    plans.sort_by_key(|p| match p.kind {
        Kind::Saver => 0,
        Kind::Balanced => 1,
        Kind::Performance => 2,
        Kind::Ultimate => 3,
        Kind::Custom => 4,
    });
    plans
}

// ---- Linux ------------------------------------------------------------------------------------

fn linux() -> Result<Report> {
    let out = process::run("powerprofilesctl", &["list"])
        .map_err(|_| EngineError::Refused(NO_PROFILES))?;
    if !out.success() {
        return Err(EngineError::Refused(NO_PROFILES));
    }
    Ok(Report {
        plans: parse_powerprofiles(&out.stdout),
        mode_plugged_in: None,
        mode_on_battery: None,
    })
}

/// `powerprofilesctl list`: a profile per unindented-ish `name:` line, `*` before the active
/// one, and its details indented below.
pub fn parse_powerprofiles(output: &str) -> Vec<Plan> {
    let mut plans: Vec<Plan> = Vec::new();
    for line in output.lines() {
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if indent <= 2 {
            let (active, rest) = match trimmed.strip_prefix('*') {
                Some(rest) => (true, rest.trim()),
                None => (false, trimmed),
            };
            let Some(name) = rest.strip_suffix(':') else {
                continue;
            };
            let kind = match name {
                "power-saver" => Kind::Saver,
                "balanced" => Kind::Balanced,
                "performance" => Kind::Performance,
                _ => Kind::Custom,
            };
            plans.push(Plan {
                id: name.to_owned(),
                name: name.to_owned(),
                kind,
                active,
                degraded: None,
            });
        } else if let Some((key, value)) = trimmed.split_once(':')
            && key.trim() == "Degraded"
            && let Some(plan) = plans.last_mut()
        {
            let value = value.trim();
            if !value.is_empty() && value != "no" {
                plan.degraded = Some(value.to_owned());
            }
        }
    }
    in_order(plans)
}
