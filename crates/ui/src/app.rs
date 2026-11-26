//! Window layout: navigation on the left, the current page in the middle, status at the
//! bottom. Pages change the system only through engine requests.

use crate::jobs::{self, Job};
use crate::pages;
use crate::theme;
use crate::widgets;
use egui::{CornerRadius, Margin, RichText};
use rewind_os_engine::system::Overview;
use rewind_os_engine::{Engine, Os, Request, Response};
use rewind_os_i18n::tr;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Overview,
    Analyzer,
    Duplicates,
    Cleanup,
    Leftovers,
    BuildFolders,
    VirtualDisks,
    Programs,
    Startup,
    Lighten,
    Maintenance,
    Disks,
    Processes,
}

/// Navigation groups, in the order the sidebar shows them.
const SECTIONS: [(&str, &[Page]); 4] = [
    ("", &[Page::Overview]),
    (
        "Free up space",
        &[
            Page::Analyzer,
            Page::Duplicates,
            Page::Cleanup,
            Page::Leftovers,
            Page::BuildFolders,
            Page::VirtualDisks,
        ],
    ),
    (
        "Programs",
        &[Page::Programs, Page::Startup, Page::Processes],
    ),
    ("System", &[Page::Lighten, Page::Maintenance, Page::Disks]),
];

impl Page {
    const ALL: [Page; 13] = [
        Page::Overview,
        Page::Analyzer,
        Page::Duplicates,
        Page::Cleanup,
        Page::Leftovers,
        Page::BuildFolders,
        Page::VirtualDisks,
        Page::Programs,
        Page::Startup,
        Page::Lighten,
        Page::Maintenance,
        Page::Disks,
        Page::Processes,
    ];

    /// The tools behind the page; it is available when any of them works on this system.
    fn tools(self) -> &'static [&'static str] {
        match self {
            Page::Overview => &["system.overview"],
            Page::Analyzer => &["disk.scan"],
            Page::Duplicates => &["disk.duplicates"],
            Page::Cleanup => &["cleanup.analyze"],
            Page::Leftovers => &["leftovers.find"],
            Page::BuildFolders => &["dev.find"],
            Page::VirtualDisks => &["vdisk.list"],
            Page::Programs => &["programs.list"],
            Page::Startup => &["startup.list"],
            Page::Lighten => &["tweaks.list"],
            Page::Maintenance => &[
                "maintenance.analyze_component_store",
                "maintenance.journal_usage",
            ],
            Page::Disks => &["disk.health"],
            Page::Processes => &["processes.list"],
        }
    }

    pub fn available(self) -> bool {
        self.tools()
            .iter()
            .any(|id| rewind_os_engine::tool(id).is_some_and(|t| t.available()))
    }

    pub fn title(self) -> &'static str {
        match self {
            Page::Overview => "Home",
            Page::Analyzer => "Disk analyzer",
            Page::Duplicates => "Duplicate files",
            Page::Cleanup => "Junk cleanup",
            Page::Leftovers => "Program leftovers",
            Page::BuildFolders => "Build folders",
            Page::VirtualDisks => "Virtual disks",
            Page::Programs => "Installed programs",
            Page::Startup => "Startup programs",
            Page::Lighten => "Make Windows lighter",
            Page::Maintenance => "Maintenance",
            Page::Disks => "Disk health",
            Page::Processes => "Running programs",
        }
    }

    /// A one-line hint shown under the title in the sidebar tooltip.
    fn glyph(self) -> crate::glyphs::Glyph {
        use crate::glyphs::Glyph;
        match self {
            Page::Overview => Glyph::Home,
            Page::Analyzer => Glyph::Analyzer,
            Page::Duplicates => Glyph::Duplicates,
            Page::Cleanup => Glyph::Cleanup,
            Page::Leftovers => Glyph::Leftovers,
            Page::BuildFolders => Glyph::BuildFolders,
            Page::VirtualDisks => Glyph::Stack,
            Page::Programs => Glyph::Programs,
            Page::Startup => Glyph::Startup,
            Page::Lighten => Glyph::Lighten,
            Page::Maintenance => Glyph::Maintenance,
            Page::Disks => Glyph::Health,
            Page::Processes => Glyph::Gauge,
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Page::Overview => "Recommendations and your drives",
            Page::Analyzer => "See what takes up space, folder by folder",
            Page::Duplicates => "Find files you have more than once",
            Page::Cleanup => "Delete temporary files and caches",
            Page::Leftovers => "Folders left by uninstalled programs",
            Page::BuildFolders => {
                "node_modules, target and other folders your projects can rebuild"
            }
            Page::VirtualDisks => "WSL and Docker disks that kept space Windows can have back",
            Page::Programs => "Uninstall what you no longer use",
            Page::Startup => "Choose what starts with the system",
            Page::Lighten => "Turn off ads, tracking and extras you do not use",
            Page::Maintenance => "The system's own maintenance tools",
            Page::Disks => "Whether your drives are failing, and keeping them optimized",
            Page::Processes => "What slows the computer down right now",
        }
    }
}

/// The app icon for the top bar, from the 64 px render.
fn load_logo(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let png = include_bytes!(
        "../../../assets/app-icon/hicolor/64x64/apps/io.github.rewind_os.rewind_os_app.png"
    );
    let icon = eframe::icon_data::from_png_bytes(png).ok()?;
    let size = [icon.width as usize, icon.height as usize];
    let image = egui::ColorImage::from_rgba_unmultiplied(size, &icon.rgba);
    Some(ctx.load_texture("app-logo", image, egui::TextureOptions::LINEAR))
}

/// Restarting elevated is a Windows feature; elsewhere the CLI runs under sudo instead.
/// Linux and macOS ask for the password for one action instead of restarting the app.
pub fn can_elevate_actions() -> bool {
    static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        rewind_os_engine::tool("system.run_elevated").is_some_and(|t| t.available())
            && rewind_os_engine::elevated::available()
    })
}

fn can_relaunch() -> bool {
    rewind_os_engine::tool("system.relaunch_elevated").is_some_and(|t| t.available())
}

pub struct Status {
    pub text: String,
    pub error: bool,
}

/// State every page can read and write.
pub struct Shared {
    pub engine: Arc<Engine>,
    pub elevated: bool,
    pub overview: Option<Overview>,
    pub status: Option<Status>,
    /// Set by a page to switch pages at the end of the frame.
    pub goto: Option<Page>,
    /// Set by the overview to start a scan in the analyzer, or a search on the duplicates page.
    pub scan_request: Option<PathBuf>,
    /// Set by a recommendation: the cleanup categories to select.
    pub cleanup_request: Option<Vec<String>>,
    /// Search for program leftovers when the page opens.
    pub leftovers_request: bool,
    pub relaunch: Option<Job>,
    /// Program icons, shared by the pages that list programs.
    pub icons: crate::icons::IconCache,
}

impl Shared {
    /// `request` as it should run: with administrator rights through the system's password
    /// prompt when the app is not elevated and the request may run that way.
    pub fn with_rights(&self, request: Request) -> Request {
        if !self.elevated
            && can_elevate_actions()
            && rewind_os_engine::elevated::ALLOWED.contains(&request.id())
        {
            Request::RunElevated {
                request: Box::new(request),
            }
        } else {
            request
        }
    }

    /// Shows `path` in the file manager. Says so when it is gone and the nearest folder that
    /// still exists was opened instead.
    pub fn reveal(&mut self, path: std::path::PathBuf) {
        match self.engine.run(
            rewind_os_engine::Request::FilesReveal { path },
            &rewind_os_engine::Task::new(),
        ) {
            Ok(rewind_os_engine::Response::Done(message))
                if message == rewind_os_engine::REVEALED_NEAREST =>
            {
                self.info(rewind_os_i18n::tr(message));
            }
            Ok(_) => {}
            Err(err) => self.error(crate::widgets::error_text(&err)),
        }
    }

    pub fn info(&mut self, text: impl Into<String>) {
        self.status = Some(Status {
            text: text.into(),
            error: false,
        });
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.status = Some(Status {
            text: text.into(),
            error: true,
        });
    }

    /// Starts an elevated copy of the app; the window closes once it started.
    pub fn relaunch_elevated(&mut self, ctx: &egui::Context) {
        if self.relaunch.is_none() {
            self.relaunch = Some(Job::start(
                ctx,
                &self.engine,
                Request::RelaunchElevated { args: Vec::new() },
            ));
        }
    }

    /// A short "needs administrator" note with a restart button, for pages and cards.
    pub fn admin_hint(&mut self, ui: &mut egui::Ui) {
        let t = theme::current(ui.ctx());
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(tr("This needs administrator rights.")).color(t.warn));
            if can_relaunch() {
                if ui.button(tr("Restart as administrator")).clicked() {
                    self.relaunch_elevated(ui.ctx());
                }
            } else if can_elevate_actions() {
                ui.label(
                    RichText::new(tr(
                        "The system asks for your password when you start one of these actions.",
                    ))
                    .color(t.text_muted),
                );
            } else {
                ui.label(
                    RichText::new(tr(
                        "Run the command-line tool with sudo for these: sudo rewind-os-cli",
                    ))
                    .color(t.text_muted),
                );
            }
        });
    }
}

pub struct App {
    shared: Shared,
    page: Page,
    overview: pages::overview::State,
    analyzer: pages::analyzer::State,
    duplicates: pages::duplicates::State,
    cleanup: pages::cleanup::State,
    leftovers: pages::leftovers::State,
    build_folders: pages::build_folders::State,
    vdisks: pages::vdisks::State,
    programs: pages::programs::State,
    startup: pages::startup::State,
    lighten: pages::lighten::State,
    maintenance: pages::maintenance::State,
    disks: pages::disks::State,
    processes: pages::processes::State,
    devtools: crate::devtools::Devtools,
    logo: Option<egui::TextureHandle>,
    update: crate::update::Notice,
}

impl App {
    pub fn new(ctx: &egui::Context) -> Self {
        let engine = Arc::new(Engine::new());
        let overview = pages::overview::State::new(ctx, &engine);
        let update = crate::update::Notice::new(ctx, &engine);
        let page = crate::devtools::start_page()
            .and_then(|name| {
                Page::ALL
                    .into_iter()
                    .find(|p| format!("{p:?}").eq_ignore_ascii_case(&name))
            })
            .unwrap_or(Page::Overview);
        Self {
            shared: Shared {
                engine,
                elevated: rewind_os_engine::system::is_elevated(),
                overview: None,
                status: None,
                goto: None,
                scan_request: crate::devtools::scan_path(),
                cleanup_request: None,
                leftovers_request: false,
                relaunch: None,
                icons: Default::default(),
            },
            page,
            overview,
            analyzer: Default::default(),
            duplicates: Default::default(),
            cleanup: Default::default(),
            leftovers: Default::default(),
            build_folders: Default::default(),
            vdisks: Default::default(),
            programs: Default::default(),
            startup: Default::default(),
            lighten: Default::default(),
            maintenance: Default::default(),
            disks: Default::default(),
            processes: Default::default(),
            devtools: crate::devtools::Devtools::from_env(),
            logo: load_logo(ctx),
            update,
        }
    }

    fn busy(&self) -> bool {
        self.overview.busy()
            || self.analyzer.busy()
            || self.duplicates.busy()
            || self.cleanup.busy()
            || self.leftovers.busy()
            || self.build_folders.busy()
            || self.vdisks.busy()
            || self.programs.busy()
            || self.startup.busy()
            || self.lighten.busy()
            || self.maintenance.busy()
            || self.disks.busy()
            || self.processes.busy()
    }

    /// The left column: the app's name, the pages, and the rights the app runs with.
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let t = theme::current(ui.ctx());
        self.update.poll(ui.ctx(), &mut self.shared);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            if let Some(logo) = &self.logo {
                ui.add(
                    egui::Image::new((logo.id(), egui::vec2(26.0, 26.0)))
                        .corner_radius(CornerRadius::same(6))
                        .alt_text("Rewind OS"),
                );
            }
            ui.label(
                RichText::new("Rewind OS")
                    .size(17.0)
                    .strong()
                    .family(crate::theme::bold()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.update.version_menu(ui, &mut self.shared);
            });
        });
        // The update notice and the rights stay at the bottom, in a panel of their own laid out
        // top to bottom; the pages scroll above them when the window is too short for all.
        egui::Panel::bottom("sidebar_footer")
            .frame(egui::Frame::NONE)
            .show_separator_line(false)
            .resizable(false)
            .show(ui, |ui| {
                self.update.notice(ui, &mut self.shared);
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    if self.shared.elevated {
                        ui.label(RichText::new(tr("Administrator")).small().color(t.text_muted));
                    } else if can_relaunch() {
                        let restart = egui::Button::new(
                            RichText::new(tr("Restart as administrator")).small().color(t.accent),
                        )
                        .frame(false);
                        if ui
                            .add(restart)
                            .on_hover_text(tr(
                                "Some cleanups and settings need administrator rights. Windows will ask for permission.",
                            ))
                            .clicked()
                        {
                            self.shared.relaunch_elevated(ui.ctx());
                        }
                    } else {
                        ui.label(RichText::new(tr("Standard user")).small().color(t.text_muted));
                    }
                });
                ui.add_space(6.0);
            });
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| self.navigation(ui));
    }

    /// The pages, in their groups.
    fn navigation(&mut self, ui: &mut egui::Ui) {
        let t = theme::current(ui.ctx());
        ui.add_space(18.0);
        for (heading, pages) in SECTIONS {
            if !heading.is_empty() {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.label(RichText::new(tr(heading)).size(12.0).color(t.text_muted));
                });
                ui.add_space(2.0);
            }
            for &page in pages {
                let available = page.available();
                let response = widgets::nav_item(
                    ui,
                    page.glyph(),
                    tr(page.title()),
                    self.page == page,
                    available,
                );
                let response = if available {
                    response.on_hover_text(tr(page.hint()))
                } else {
                    response.on_hover_text(rewind_os_i18n::trf(
                        "Not available on {os} yet.",
                        &[("os", Os::current().name())],
                    ))
                };
                if response.clicked() {
                    self.page = page;
                }
            }
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let t = theme::current(ui.ctx());
        if let Some(status) = &self.shared.status {
            let color = if status.error { t.danger } else { t.text_muted };
            // Messages say what finished; only errors need a word in front.
            let text = if status.error {
                format!("{}: {}", tr("Error"), status.text)
            } else {
                status.text.clone()
            };
            let response = ui.label(RichText::new(text).small().color(color));
            // A live region: screen readers announce each new message without moving focus,
            // so the end of a background job is heard, not only seen.
            let live = if status.error {
                egui::accesskit::Live::Assertive
            } else {
                egui::accesskit::Live::Polite
            };
            ui.ctx()
                .accesskit_node_builder(response.id, |node| node.set_live(live));
        }
    }

    fn poll_relaunch(&mut self, ctx: &egui::Context) {
        if let Some(outcome) = jobs::finished(&mut self.shared.relaunch, ctx) {
            match outcome {
                Ok(Response::Done(_)) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                Ok(_) => {}
                Err(err) => self.shared.error(format!(
                    "{} ({err})",
                    tr("Could not restart as administrator")
                )),
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let t = theme::current(&ctx);
        self.poll_relaunch(&ctx);
        self.overview.poll(&ctx, &mut self.shared);

        if self.shared.status.is_some() {
            egui::Panel::bottom("status_bar")
                .frame(
                    egui::Frame::new()
                        .fill(t.background)
                        .inner_margin(Margin::symmetric(28, 10)),
                )
                .show(ui, |ui| self.status_bar(ui));
        }
        egui::Panel::left("navigation")
            .exact_size(224.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(t.sidebar)
                    .inner_margin(Margin::symmetric(12, 18)),
            )
            .show(ui, |ui| self.sidebar(ui));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(t.background)
                    .inner_margin(Margin {
                        left: 24,
                        right: 24,
                        top: 18,
                        bottom: 16,
                    })
                    .corner_radius(CornerRadius::ZERO),
            )
            .show(ui, |ui| {
                let shared = &mut self.shared;
                match self.page {
                    Page::Overview => self.overview.ui(ui, shared),
                    Page::Analyzer => self.analyzer.ui(ui, shared),
                    Page::Duplicates => self.duplicates.ui(ui, shared),
                    Page::Cleanup => self.cleanup.ui(ui, shared),
                    Page::Leftovers => self.leftovers.ui(ui, shared),
                    Page::BuildFolders => self.build_folders.ui(ui, shared),
                    Page::VirtualDisks => self.vdisks.ui(ui, shared),
                    Page::Programs => self.programs.ui(ui, shared),
                    Page::Startup => self.startup.ui(ui, shared),
                    Page::Lighten => self.lighten.ui(ui, shared),
                    Page::Maintenance => self.maintenance.ui(ui, shared),
                    Page::Disks => self.disks.ui(ui, shared),
                    Page::Processes => self.processes.ui(ui, shared),
                }
            });
        if let Some(page) = self.shared.goto.take() {
            self.page = page;
        }
        let busy = self.busy();
        self.devtools.frame(&ctx, busy);
    }
}
