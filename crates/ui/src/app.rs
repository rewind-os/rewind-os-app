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
    Cleanup,
    Leftovers,
    Programs,
    Startup,
    Lighten,
    Maintenance,
}

/// Navigation groups, in the order the sidebar shows them.
const SECTIONS: [(&str, &[Page]); 4] = [
    ("", &[Page::Overview]),
    (
        "Free up space",
        &[Page::Analyzer, Page::Cleanup, Page::Leftovers],
    ),
    ("Programs", &[Page::Programs, Page::Startup]),
    ("System", &[Page::Lighten, Page::Maintenance]),
];

impl Page {
    const ALL: [Page; 8] = [
        Page::Overview,
        Page::Analyzer,
        Page::Cleanup,
        Page::Leftovers,
        Page::Programs,
        Page::Startup,
        Page::Lighten,
        Page::Maintenance,
    ];

    /// The tools behind the page; it is available when any of them works on this system.
    fn tools(self) -> &'static [&'static str] {
        match self {
            Page::Overview => &["system.overview"],
            Page::Analyzer => &["disk.scan"],
            Page::Cleanup => &["cleanup.analyze"],
            Page::Leftovers => &["leftovers.find"],
            Page::Programs => &["programs.list"],
            Page::Startup => &["startup.list"],
            Page::Lighten => &["tweaks.list"],
            Page::Maintenance => &[
                "maintenance.analyze_component_store",
                "maintenance.journal_usage",
            ],
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
            Page::Cleanup => "Junk cleanup",
            Page::Leftovers => "Program leftovers",
            Page::Programs => "Installed programs",
            Page::Startup => "Startup programs",
            Page::Lighten => "Make Windows lighter",
            Page::Maintenance => "Maintenance",
        }
    }

    /// A one-line hint shown under the title in the sidebar tooltip.
    pub fn hint(self) -> &'static str {
        match self {
            Page::Overview => "Recommendations and your drives",
            Page::Analyzer => "See what takes up space, folder by folder",
            Page::Cleanup => "Delete temporary files and caches",
            Page::Leftovers => "Folders left by uninstalled programs",
            Page::Programs => "Uninstall what you no longer use",
            Page::Startup => "Choose what starts with the system",
            Page::Lighten => "Turn off ads, tracking and extras you do not use",
            Page::Maintenance => "The system's own maintenance tools",
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
    /// Set by the overview to start a scan in the analyzer.
    pub scan_request: Option<PathBuf>,
    /// Set by a recommendation: the cleanup categories to select.
    pub cleanup_request: Option<Vec<String>>,
    pub relaunch: Option<Job>,
}

impl Shared {
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
    cleanup: pages::cleanup::State,
    leftovers: pages::leftovers::State,
    programs: pages::programs::State,
    startup: pages::startup::State,
    lighten: pages::lighten::State,
    maintenance: pages::maintenance::State,
    devtools: crate::devtools::Devtools,
    logo: Option<egui::TextureHandle>,
}

impl App {
    pub fn new(ctx: &egui::Context) -> Self {
        let engine = Arc::new(Engine::new());
        let overview = pages::overview::State::new(ctx, &engine);
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
                relaunch: None,
            },
            page,
            overview,
            analyzer: Default::default(),
            cleanup: Default::default(),
            leftovers: Default::default(),
            programs: Default::default(),
            startup: Default::default(),
            lighten: Default::default(),
            maintenance: Default::default(),
            devtools: crate::devtools::Devtools::from_env(),
            logo: load_logo(ctx),
        }
    }

    fn busy(&self) -> bool {
        self.overview.busy()
            || self.analyzer.busy()
            || self.cleanup.busy()
            || self.leftovers.busy()
            || self.programs.busy()
            || self.startup.busy()
            || self.lighten.busy()
            || self.maintenance.busy()
    }

    /// The left column: the app's name, the pages, and the rights the app runs with.
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let t = theme::current(ui.ctx());
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
            ui.label(RichText::new("Rewind OS").size(17.0).strong());
        });
        ui.add_space(22.0);
        for (heading, pages) in SECTIONS {
            if !heading.is_empty() {
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.label(
                        RichText::new(tr(heading).to_uppercase())
                            .size(11.0)
                            .color(t.text_muted),
                    );
                });
                ui.add_space(2.0);
            }
            for &page in pages {
                let available = page.available();
                let response =
                    widgets::nav_item(ui, tr(page.title()), self.page == page, available);
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
        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.add_space(6.0);
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
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let t = theme::current(ui.ctx());
        if let Some(status) = &self.shared.status {
            let color = if status.error { t.danger } else { t.text_muted };
            let prefix = if status.error {
                tr("Error")
            } else {
                tr("Done")
            };
            ui.label(
                RichText::new(format!("{prefix}: {}", status.text))
                    .small()
                    .color(color),
            );
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
                    .fill(t.background)
                    .inner_margin(Margin::symmetric(12, 18)),
            )
            .show(ui, |ui| self.sidebar(ui));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(t.background)
                    .inner_margin(Margin {
                        left: 20,
                        right: 32,
                        top: 22,
                        bottom: 16,
                    })
                    .corner_radius(CornerRadius::ZERO),
            )
            .show(ui, |ui| {
                let shared = &mut self.shared;
                match self.page {
                    Page::Overview => self.overview.ui(ui, shared),
                    Page::Analyzer => self.analyzer.ui(ui, shared),
                    Page::Cleanup => self.cleanup.ui(ui, shared),
                    Page::Leftovers => self.leftovers.ui(ui, shared),
                    Page::Programs => self.programs.ui(ui, shared),
                    Page::Startup => self.startup.ui(ui, shared),
                    Page::Lighten => self.lighten.ui(ui, shared),
                    Page::Maintenance => self.maintenance.ui(ui, shared),
                }
            });
        if let Some(page) = self.shared.goto.take() {
            self.page = page;
        }
        let busy = self.busy();
        self.devtools.frame(&ctx, busy);
    }
}
