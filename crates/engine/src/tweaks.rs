//! Making Windows lighter: a closed catalog of settings, each one documented by Microsoft as a
//! registry value or policy, and each one fully reversible. Nothing is uninstalled or deleted;
//! turning a setting back off restores Windows' default (the value it had, or no value).
//!
//! If writing a setting fails halfway, the values already written are put back as they were,
//! so a setting is never left half applied.

use crate::{EngineError, Result};
use rewind_os_platform::elevation;
use rewind_os_platform::registry::{self, Hive, Value};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    Suggestions,
    Privacy,
    Features,
    Performance,
}

impl Group {
    pub const ALL: [Group; 4] = [
        Group::Suggestions,
        Group::Privacy,
        Group::Features,
        Group::Performance,
    ];

    /// English label; translate with `rewind_os_i18n::tr`.
    pub fn label(self) -> &'static str {
        match self {
            Group::Suggestions => "Ads and suggestions",
            Group::Privacy => "Privacy",
            Group::Features => "Features you may not use",
            Group::Performance => "Visual effects",
        }
    }
}

/// When a change is noticed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Takes {
    Immediately,
    SignOut,
    Restart,
}

impl Takes {
    /// English text; translate with `rewind_os_i18n::tr`.
    pub fn label(self) -> &'static str {
        match self {
            Takes::Immediately => "Takes effect right away",
            Takes::SignOut => "Takes effect after signing out",
            Takes::Restart => "Takes effect after restarting",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Data {
    Dword(u32),
    Text(&'static str),
}

/// How a value goes back to Windows' default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Undo {
    /// Remove the value (policies: no value means the default).
    Delete,
    /// Write Windows' default value.
    Set(Data),
}

#[derive(Debug, Clone, Copy)]
pub struct Change {
    pub hive: Hive,
    pub path: &'static str,
    pub name: &'static str,
    pub on: Data,
    pub undo: Undo,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Tweak {
    pub id: &'static str,
    /// English name and description; translate with `rewind_os_i18n::tr`.
    pub name: &'static str,
    pub description: &'static str,
    pub group: Group,
    /// Safe for almost everyone; offered by "Apply recommended".
    pub recommended: bool,
    pub takes: Takes,
    #[serde(skip)]
    pub changes: &'static [Change],
}

impl Tweak {
    pub fn needs_admin(&self) -> bool {
        self.changes.iter().any(|c| c.hive == Hive::LocalMachine)
    }
}

const fn hkcu(path: &'static str, name: &'static str, on: u32, default: u32) -> Change {
    Change {
        hive: Hive::CurrentUser,
        path,
        name,
        on: Data::Dword(on),
        undo: Undo::Set(Data::Dword(default)),
    }
}

const fn policy(hive: Hive, path: &'static str, name: &'static str, on: u32) -> Change {
    Change {
        hive,
        path,
        name,
        on: Data::Dword(on),
        undo: Undo::Delete,
    }
}

const CDM: &str = r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager";

pub const CATALOG: &[Tweak] = &[
    Tweak {
        id: "suggestions.start",
        name: "Suggestions in Start and Settings",
        description: "Stops Windows from showing suggested apps and offers in the Start menu and in Settings.",
        group: Group::Suggestions,
        recommended: true,
        takes: Takes::SignOut,
        changes: &[
            hkcu(CDM, "SystemPaneSuggestionsEnabled", 0, 1),
            hkcu(CDM, "SubscribedContent-338388Enabled", 0, 1),
            hkcu(CDM, "SubscribedContent-338393Enabled", 0, 1),
            hkcu(CDM, "SubscribedContent-353694Enabled", 0, 1),
            hkcu(CDM, "SubscribedContent-353696Enabled", 0, 1),
        ],
    },
    Tweak {
        id: "suggestions.tips",
        name: "Tips and tricks",
        description: "Stops notifications with tips about using Windows.",
        group: Group::Suggestions,
        recommended: true,
        takes: Takes::Immediately,
        changes: &[
            hkcu(CDM, "SubscribedContent-338389Enabled", 0, 1),
            hkcu(CDM, "SoftLandingEnabled", 0, 1),
        ],
    },
    Tweak {
        id: "suggestions.auto_installed_apps",
        name: "Apps installed without asking",
        description: "Stops Windows from installing suggested apps and games on its own. Apps already installed stay.",
        group: Group::Suggestions,
        recommended: true,
        takes: Takes::Immediately,
        changes: &[
            hkcu(CDM, "SilentInstalledAppsEnabled", 0, 1),
            hkcu(CDM, "PreInstalledAppsEnabled", 0, 1),
            hkcu(CDM, "OemPreInstalledAppsEnabled", 0, 1),
        ],
    },
    Tweak {
        id: "suggestions.lock_screen",
        name: "Fun facts on the lock screen",
        description: "Removes tips and promotions shown over the lock screen picture.",
        group: Group::Suggestions,
        recommended: true,
        takes: Takes::Immediately,
        changes: &[
            hkcu(CDM, "RotatingLockScreenOverlayEnabled", 0, 1),
            hkcu(CDM, "SubscribedContent-338387Enabled", 0, 1),
        ],
    },
    Tweak {
        id: "suggestions.explorer",
        name: "Offers in File Explorer",
        description: "Stops cloud storage offers in File Explorer.",
        group: Group::Suggestions,
        recommended: true,
        takes: Takes::Immediately,
        changes: &[hkcu(
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced",
            "ShowSyncProviderNotifications",
            0,
            1,
        )],
    },
    Tweak {
        id: "suggestions.web_search",
        name: "Web results in Start search",
        description: "Start menu search shows only results from this computer, without web suggestions.",
        group: Group::Suggestions,
        recommended: true,
        takes: Takes::SignOut,
        changes: &[policy(
            Hive::CurrentUser,
            r"Software\Policies\Microsoft\Windows\Explorer",
            "DisableSearchBoxSuggestions",
            1,
        )],
    },
    Tweak {
        id: "privacy.advertising_id",
        name: "Advertising ID",
        description: "Apps can no longer use an identifier to show you personalised ads.",
        group: Group::Privacy,
        recommended: true,
        takes: Takes::Immediately,
        changes: &[hkcu(
            r"Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo",
            "Enabled",
            0,
            1,
        )],
    },
    Tweak {
        id: "privacy.tailored_experiences",
        name: "Tailored experiences",
        description: "Microsoft stops using your diagnostic data to personalise tips, ads and recommendations.",
        group: Group::Privacy,
        recommended: true,
        takes: Takes::Immediately,
        changes: &[hkcu(
            r"Software\Microsoft\Windows\CurrentVersion\Privacy",
            "TailoredExperiencesWithDiagnosticDataEnabled",
            0,
            1,
        )],
    },
    Tweak {
        id: "privacy.diagnostic_data",
        name: "Optional diagnostic data",
        description: "Windows sends only the required diagnostic data, the least it allows.",
        group: Group::Privacy,
        recommended: true,
        takes: Takes::Restart,
        changes: &[policy(
            Hive::LocalMachine,
            r"SOFTWARE\Policies\Microsoft\Windows\DataCollection",
            "AllowTelemetry",
            1,
        )],
    },
    Tweak {
        id: "privacy.activity_history",
        name: "Activity history",
        description: "Windows stops keeping and sending a history of the apps and files you use.",
        group: Group::Privacy,
        recommended: true,
        takes: Takes::SignOut,
        changes: &[
            policy(
                Hive::LocalMachine,
                r"SOFTWARE\Policies\Microsoft\Windows\System",
                "PublishUserActivities",
                0,
            ),
            policy(
                Hive::LocalMachine,
                r"SOFTWARE\Policies\Microsoft\Windows\System",
                "UploadUserActivities",
                0,
            ),
        ],
    },
    Tweak {
        id: "privacy.telemetry_service",
        name: "Telemetry service",
        description: "Turns off the Connected User Experiences and Telemetry service, which sends diagnostic data in the background.",
        group: Group::Privacy,
        recommended: false,
        takes: Takes::Restart,
        changes: &[Change {
            hive: Hive::LocalMachine,
            path: r"SYSTEM\CurrentControlSet\Services\DiagTrack",
            name: "Start",
            on: Data::Dword(4),
            undo: Undo::Set(Data::Dword(2)),
        }],
    },
    Tweak {
        id: "features.widgets",
        name: "Widgets",
        description: "Turns off the Widgets board with news and weather, and its button on the taskbar.",
        group: Group::Features,
        recommended: false,
        takes: Takes::SignOut,
        changes: &[policy(
            Hive::LocalMachine,
            r"SOFTWARE\Policies\Microsoft\Dsh",
            "AllowNewsAndInterests",
            0,
        )],
    },
    Tweak {
        id: "features.recall",
        name: "Recall snapshots",
        description: "Windows stops saving snapshots of your screen. Only Copilot+ PCs have this feature.",
        group: Group::Features,
        recommended: true,
        takes: Takes::SignOut,
        changes: &[policy(
            Hive::CurrentUser,
            r"Software\Policies\Microsoft\Windows\WindowsAI",
            "DisableAIDataAnalysis",
            1,
        )],
    },
    Tweak {
        id: "features.game_recording",
        name: "Background game recording",
        description: "Turns off Game Bar's background recording of games. You can still take screenshots.",
        group: Group::Features,
        recommended: false,
        takes: Takes::Immediately,
        changes: &[
            hkcu(r"System\GameConfigStore", "GameDVR_Enabled", 0, 1),
            hkcu(
                r"Software\Microsoft\Windows\CurrentVersion\GameDVR",
                "AppCaptureEnabled",
                0,
                1,
            ),
        ],
    },
    Tweak {
        id: "features.update_sharing",
        name: "Sharing updates with other computers",
        description: "Windows downloads updates only from Microsoft and stops uploading them to other computers.",
        group: Group::Features,
        recommended: false,
        takes: Takes::Restart,
        changes: &[policy(
            Hive::LocalMachine,
            r"SOFTWARE\Policies\Microsoft\Windows\DeliveryOptimization",
            "DODownloadMode",
            0,
        )],
    },
    Tweak {
        id: "performance.transparency",
        name: "Transparency effects",
        description: "Windows, the taskbar and Start become solid instead of see-through. Lighter on older computers.",
        group: Group::Performance,
        recommended: false,
        takes: Takes::Immediately,
        changes: &[hkcu(
            r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "EnableTransparency",
            0,
            1,
        )],
    },
    Tweak {
        id: "performance.animations",
        name: "Window animations",
        description: "Windows open and minimise instantly, without animation.",
        group: Group::Performance,
        recommended: false,
        takes: Takes::SignOut,
        changes: &[Change {
            hive: Hive::CurrentUser,
            path: r"Control Panel\Desktop\WindowMetrics",
            name: "MinAnimate",
            on: Data::Text("0"),
            undo: Undo::Set(Data::Text("1")),
        }],
    },
];

pub fn tweak(id: &str) -> Option<&'static Tweak> {
    CATALOG.iter().find(|t| t.id == id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Applied,
    /// Windows' default.
    Default,
    /// Some of its values are set and some are not (changed by something else).
    Partial,
}

#[derive(Debug, Clone, Serialize)]
pub struct TweakStatus {
    pub tweak: Tweak,
    pub state: State,
    pub needs_admin: bool,
}

/// Where settings are read and written; the registry in the app, memory in tests.
pub trait Store {
    fn read(&self, change: &Change) -> Option<Data>;
    fn write(&self, change: &Change, data: Data) -> Result<()>;
    fn delete(&self, change: &Change) -> Result<()>;
}

/// The Windows registry.
pub struct Registry;

impl Store for Registry {
    fn read(&self, c: &Change) -> Option<Data> {
        match registry::value(c.hive, c.path, c.name)? {
            Value::Number(n) => u32::try_from(n).ok().map(Data::Dword),
            // Only the catalog's own texts matter; keep them static.
            Value::Text(t) => match c.on {
                Data::Text(on) if on == t => Some(c.on),
                _ => match c.undo {
                    Undo::Set(Data::Text(off)) if off == t => Some(Data::Text(off)),
                    _ => Some(Data::Text("")),
                },
            },
            _ => None,
        }
    }

    fn write(&self, c: &Change, data: Data) -> Result<()> {
        let result = match data {
            Data::Dword(v) => registry::set_dword(c.hive, c.path, c.name, v),
            Data::Text(t) => registry::set_text(c.hive, c.path, c.name, t),
        };
        result.map_err(to_engine)
    }

    fn delete(&self, c: &Change) -> Result<()> {
        registry::delete_value(c.hive, c.path, c.name).map_err(to_engine)
    }
}

fn to_engine(e: rewind_os_platform::PlatformError) -> EngineError {
    match e {
        rewind_os_platform::PlatformError::NeedsAdmin => EngineError::NeedsAdmin,
        other => other.into(),
    }
}

fn is_applied(store: &dyn Store, c: &Change) -> bool {
    store.read(c) == Some(c.on)
}

fn is_default(store: &dyn Store, c: &Change) -> bool {
    match (store.read(c), c.undo) {
        (None, _) => true,
        (Some(v), Undo::Set(default)) => v == default,
        (Some(_), Undo::Delete) => false,
    }
}

pub fn state(store: &dyn Store, tweak: &Tweak) -> State {
    if tweak.changes.iter().all(|c| is_applied(store, c)) {
        State::Applied
    } else if tweak.changes.iter().all(|c| is_default(store, c)) {
        State::Default
    } else {
        State::Partial
    }
}

pub fn list() -> Vec<TweakStatus> {
    list_in(&Registry)
}

pub fn list_in(store: &dyn Store) -> Vec<TweakStatus> {
    CATALOG
        .iter()
        .map(|t| TweakStatus {
            tweak: *t,
            state: state(store, t),
            needs_admin: t.needs_admin(),
        })
        .collect()
}

/// Applies (`on`) or undoes (`!on`) the tweak with `id`.
pub fn set(id: &str, on: bool) -> Result<()> {
    let tweak = tweak(id).ok_or_else(|| EngineError::NotFound(id.to_owned()))?;
    if tweak.needs_admin() && !elevation::is_elevated() {
        return Err(EngineError::NeedsAdmin);
    }
    set_in(&Registry, tweak, on)
}

/// Writes every value of `tweak`, or none: on a failure the values already written go back.
pub fn set_in(store: &dyn Store, tweak: &Tweak, on: bool) -> Result<()> {
    let before: Vec<Option<Data>> = tweak.changes.iter().map(|c| store.read(c)).collect();
    for (done, change) in tweak.changes.iter().enumerate() {
        let result = match (on, change.undo) {
            (true, _) => store.write(change, change.on),
            (false, Undo::Delete) => store.delete(change),
            (false, Undo::Set(default)) => store.write(change, default),
        };
        if let Err(err) = result {
            for (c, previous) in tweak.changes.iter().zip(&before).take(done) {
                let _ = match previous {
                    Some(v) => store.write(c, *v),
                    None => store.delete(c),
                };
            }
            return Err(err);
        }
    }
    Ok(())
}
