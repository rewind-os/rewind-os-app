//! Window layout: navigation on the left, the current page in the middle, status at the
//! bottom. Pages change the system only through engine requests.

use crate::jobs::{self, Job};
use crate::pages;
use crate::theme;
use crate::widgets;
use egui::{CornerRadius, Margin, RichText, Stroke};
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
    Maintenance,
}

impl Page {
    const ALL: [Page; 7] = [
        Page::Overview,
        Page::Analyzer,
        Page::Cleanup,
        Page::Leftovers,
        Page::Programs,
        Page::Startup,
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
            Page::Overview => "Overview",
            Page::Analyzer => "Disk analyzer",
            Page::Cleanup => "Junk cleanup",
            Page::Leftovers => "Program leftovers",
            Page::Programs => "Installed programs",
            Page::Startup => "Startup programs",
            Page::Maintenance => "Maintenance",
        }
    }
}

/// The app icon for the top bar, from the 64 px render.
fn load_logo(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let png =
        include_bytes!("../../../assets/app-icon/hicolor/64x64/apps/io.github.r_dvl.rewind_os.png");
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
                relaunch: None,
            },
            page,
            overview,
            analyzer: Default::default(),
            cleanup: Default::default(),
            leftovers: Default::default(),
            programs: Default::default(),
            startup: Default::default(),
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
            || self.maintenance.busy()
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let t = theme::current(ui.ctx());
        ui.horizontal(|ui| {
            if let Some(logo) = &self.logo {
                ui.add(egui::Image::new((logo.id(), egui::vec2(22.0, 22.0))).alt_text("Rewind OS"));
            }
            ui.label(RichText::new("Rewind OS").strong().size(16.0).color(t.accent));
            ui.label(RichText::new(tr(self.page.title())).color(t.text_muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.shared.elevated {
                    widgets::badge(ui, tr("Administrator"), t.ok);
                } else {
                    if can_relaunch()
                        && ui
                        .button(tr("Restart as administrator"))
                        .on_hover_text(tr(
                            "Some cleanups and settings need administrator rights. Windows will ask for permission.",
                        ))
                        .clicked()
                    {
                        self.shared.relaunch_elevated(ui.ctx());
                    }
                    widgets::badge(ui, tr("Standard user"), t.text_muted);
                }
            });
        });
    }

    fn navigation(&mut self, ui: &mut egui::Ui) {
        let t = theme::current(ui.ctx());
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
            for page in Page::ALL {
                let selected = self.page == page;
                let text = RichText::new(tr(page.title()));
                let text = if selected {
                    text.strong().color(t.accent)
                } else {
                    text
                };
                let button =
                    egui::Button::selectable(selected, text).min_size(egui::vec2(0.0, 34.0));
                let response = ui
                    .add_enabled(page.available(), button)
                    .on_disabled_hover_text(rewind_os_i18n::trf(
                        "Not available on {os} yet.",
                        &[("os", Os::current().name())],
                    ));
                if response.clicked() {
                    self.page = page;
                }
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let t = theme::current(ui.ctx());
        ui.horizontal(|ui| match &self.shared.status {
            Some(status) => {
                let color = if status.error { t.danger } else { t.text_muted };
                let prefix = if status.error {
                    tr("Error")
                } else {
                    tr("Done")
                };
                ui.label(RichText::new(format!("{prefix}: {}", status.text)).color(color));
            }
            None => {
                ui.label(RichText::new(tr("Ready")).color(t.text_muted));
            }
        });
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

        let bar = egui::Frame::new()
            .fill(t.surface)
            .inner_margin(Margin::symmetric(14, 8))
            .stroke(Stroke::new(1.0, t.border));
        egui::Panel::top("top_bar")
            .frame(bar)
            .show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("status_bar")
            .frame(bar)
            .show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("navigation")
            .exact_size(210.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(t.surface)
                    .inner_margin(Margin::same(8))
                    .stroke(Stroke::new(1.0, t.border)),
            )
            .show(ui, |ui| self.navigation(ui));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(t.background)
                    .inner_margin(Margin::same(18))
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
