//! The window: one tab per repository, and the bar to open more.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use egui_dock::tab_viewer::OnCloseResponse;
use egui_dock::{DockArea, DockState, Style, TabViewer};
use zrush_core::config::{Config, Dirs};
use zrush_core::host::Host;

use crate::tab::{REFRESH_EVERY, Tab, Wake};

/// The bar along the top, as tall as macOS draws its title bar's buttons.
const BAR_HEIGHT: f32 = 28.0;

/// Builds the host a tab's service drives: real processes in the app, a
/// recording one in tests.
pub type HostFactory = Box<dyn Fn(&Config) -> Box<dyn Host>>;

pub struct GuiApp {
    dirs: Dirs,
    cfg: Config,
    dock: DockState<Tab>,
    host: HostFactory,
    wake: Wake,
    /// What is typed in the "open a repository" field.
    new_repo: String,
    /// A problem worth showing above the tabs until dismissed.
    banner: Option<String>,
    /// The title last sent to the window, so it is only sent on change.
    shown_title: String,
    /// The tabs whose bodies were drawn last frame: the open ones.
    shown_tabs: Vec<PathBuf>,
    md: egui_commonmark::CommonMarkCache,
    settings: crate::settings::SettingsState,
    themes: Vec<crate::theme::Theme>,
    /// The theme name and system mode last applied, to apply on change only.
    applied: Option<(String, bool)>,
    /// The bundled fonts are in: they are set once, on the first frame.
    fonts: bool,
    palette: crate::palette::State,
    /// The app's commands live in the macOS menu bar: no bar of our own,
    /// and the tabs sit in the title bar, beside the window's buttons.
    titlebar_tabs: bool,
    #[cfg(target_os = "macos")]
    menus: Option<crate::menu::Menus>,
}

impl GuiApp {
    pub fn new(
        dirs: Dirs,
        cfg: Config,
        repos: Vec<PathBuf>,
        host: HostFactory,
        wake: Wake,
    ) -> Self {
        let mut app = Self {
            dirs,
            cfg,
            dock: DockState::new(Vec::new()),
            host,
            wake,
            new_repo: String::new(),
            banner: None,
            shown_title: String::new(),
            shown_tabs: Vec::new(),
            md: egui_commonmark::CommonMarkCache::default(),
            settings: crate::settings::SettingsState::default(),
            themes: Vec::new(),
            applied: None,
            fonts: false,
            palette: crate::palette::State::default(),
            titlebar_tabs: false,
            #[cfg(target_os = "macos")]
            menus: None,
        };
        let (themes, errors) = crate::theme::load_all(&app.dirs.themes_dir());
        app.themes = themes;
        if !errors.is_empty() {
            app.banner = Some(format!("Some themes were skipped: {}", errors.join("; ")));
        }
        for repo in repos {
            let tab = app.open_tab(&repo);
            // A saved repository that is gone still gets its tab, saying
            // why, so it can be closed; a duplicate does not.
            if !app.already_open(&tab) {
                app.dock.push_to_focused_leaf(tab);
            }
        }
        app
    }

    fn open_tab(&self, repo: &Path) -> Tab {
        Tab::open(
            repo,
            &self.dirs,
            &self.cfg,
            (self.host)(&self.cfg),
            Arc::clone(&self.wake),
        )
    }

    fn already_open(&self, tab: &Tab) -> bool {
        self.find(tab).is_some()
    }

    /// The tab already showing the same repository as `tab`: the same main
    /// worktree, whichever of its worktrees each was opened from.
    fn find(&self, tab: &Tab) -> Option<egui_dock::TabPath> {
        let root = tab.main_root()?;
        self.dock.find_tab_from(|t| t.main_root() == Some(root))
    }

    /// Open a repository in a new tab, or show the tab it already has.
    pub fn add_repo(&mut self, path: &Path) -> Result<(), String> {
        let tab = self.open_tab(path);
        if let Some(e) = tab.error {
            return Err(e);
        }
        if let Some(at) = self.find(&tab) {
            let _ = self.dock.set_active_tab(at);
            return Ok(());
        }
        self.dock.push_to_focused_leaf(tab);
        self.save_repos();
        Ok(())
    }

    /// The app's commands go in the menu bar and the tabs in the title
    /// bar, as on macOS. Set by the app, not by tests, which drive the
    /// window's own bar.
    #[must_use]
    pub fn with_titlebar_tabs(mut self) -> Self {
        self.titlebar_tabs = cfg!(target_os = "macos");
        self
    }

    /// The menu bar's items, read every frame.
    #[cfg(target_os = "macos")]
    pub fn set_menus(&mut self, menus: crate::menu::Menus) {
        self.menus = Some(menus);
    }

    /// Open the command palette, empty.
    pub fn open_palette(&mut self) {
        if !self.palette.open {
            self.palette.toggle();
        }
    }

    /// Ask for a folder and open it in a tab.
    fn open_repository(&mut self) {
        if let Some(p) = rfd::FileDialog::new().pick_folder()
            && let Err(e) = self.add_repo(&p)
        {
            self.banner = Some(format!("{}: {e}", p.display()));
        }
    }

    fn run(&mut self, c: crate::menu::Command) {
        match c {
            crate::menu::Command::OpenRepository => self.open_repository(),
            crate::menu::Command::Settings => self.open_settings(),
            crate::menu::Command::Palette => self.palette.toggle(),
        }
    }

    /// What the palette picked.
    fn pick(&mut self, target: crate::palette::Target) {
        use crate::palette::{TabAction, Target};
        match target {
            Target::App(c) => self.run(c),
            Target::Tab(repo) => self.focus(&repo),
            Target::Row(repo, row) => {
                let row = *row;
                self.focus(&repo);
                if let Some(t) = self.tabs_mut().find(|t| t.repo == repo) {
                    t.select(&row);
                }
            }
            Target::InTab(repo, action) => {
                self.focus(&repo);
                if let Some(t) = self.tabs_mut().find(|t| t.repo == repo) {
                    match action {
                        TabAction::NewWorktree => t.new_worktree(None),
                        TabAction::Purge => t.start_purge(),
                        TabAction::Refresh => t.refresh(),
                    }
                }
            }
        }
    }

    /// What was asked from the menu bar or the keyboard, done.
    fn commands(&mut self, ctx: &egui::Context) {
        #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
        let mut commands = Self::shortcuts(ctx);
        #[cfg(target_os = "macos")]
        if let Some(m) = &self.menus {
            commands.extend(m.commands());
        }
        for c in commands {
            self.run(c);
        }
    }

    fn show_palette(&mut self, ctx: &egui::Context) {
        if !self.palette.open {
            return;
        }
        let entries = crate::palette::entries(self.tabs());
        if let Some(t) = crate::palette::show(ctx, &mut self.palette, &entries) {
            self.pick(t);
        }
    }

    /// The keys the menu bar answers on macOS, answered here elsewhere and
    /// in tests; on macOS the menu takes them first.
    fn shortcuts(ctx: &egui::Context) -> Vec<crate::menu::Command> {
        use crate::menu::Command;
        ctx.input_mut(|i| {
            [
                (egui::Key::K, Command::Palette),
                (egui::Key::Comma, Command::Settings),
                (egui::Key::O, Command::OpenRepository),
            ]
            .into_iter()
            .filter(|(k, _)| i.consume_key(egui::Modifiers::COMMAND, *k))
            .map(|(_, c)| c)
            .collect()
        })
    }

    /// Bring the tab opened for `repo` to the front.
    pub fn focus(&mut self, repo: &Path) {
        if let Some(at) = self.dock.find_tab_from(|t| t.repo == repo) {
            let _ = self.dock.set_active_tab(at);
        }
    }

    pub fn close_tab_for(&mut self, repo: &Path) {
        if let Some(at) = self.dock.find_tab_from(|t| t.repo == repo) {
            self.dock.remove_tab(at);
        }
        self.save_repos();
    }

    pub fn tabs(&self) -> impl Iterator<Item = &Tab> {
        self.dock.iter_all_tabs().map(|(_, t)| t)
    }

    pub fn tabs_mut(&mut self) -> impl Iterator<Item = &mut Tab> {
        self.dock.iter_all_tabs_mut().map(|(_, t)| t)
    }

    pub fn tab_count(&self) -> usize {
        self.tabs().count()
    }

    /// Put the configured theme on, when it or the system's mode changed.
    fn apply_theme(&mut self, ctx: &egui::Context) {
        let os_dark = ctx.system_theme() != Some(egui::Theme::Light);
        let want = (self.cfg.gui_theme.clone(), os_dark);
        if self.applied.as_ref() == Some(&want) || self.themes.is_empty() {
            return;
        }
        let t = crate::theme::pick(&self.themes, &want.0, os_dark);
        crate::theme::apply(ctx, t);
        self.applied = Some(want);
    }

    pub fn open_settings(&mut self) {
        self.settings.open = true;
    }

    /// A setting changed through one tab's service: every other tab reads
    /// the file again, and all of them ask again with the new values.
    fn apply_settings(&mut self, cfg: &Config) {
        // Another agent listed: every tab switches, and drops what the one
        // it left had listed.
        let agent = cfg
            .agent
            .clone()
            .filter(|a| self.cfg.agent.as_ref() != Some(a));
        self.cfg = cfg.clone();
        for t in self.tabs_mut() {
            if let Some(z) = t.zrush() {
                let _ = z.reload_config();
                if let Some(a) = &agent {
                    let _ = z.set_active_agent(a);
                }
            }
            if agent.is_some() {
                t.snap.forget_sessions();
            }
            t.view.resumable_max = cfg.resumable_max;
            t.refresh();
        }
    }

    /// The service settings go through: the first tab whose repository
    /// opened. Any would do, they share one config file.
    fn settings_service(&self) -> Option<Arc<zrush_core::app::Zrush>> {
        self.tabs().find_map(|t| t.zrush().cloned())
    }

    pub fn window_title(&self) -> String {
        let live: usize = self.tabs().map(Tab::live_count).sum();
        if live > 0 {
            format!("zrush — ● {live} live")
        } else {
            "zrush".into()
        }
    }

    /// The open tabs, in order, into `gui_repos`. Written on top of the
    /// file as it is now, so nothing else in it is touched.
    fn save_repos(&mut self) {
        let repos: Vec<PathBuf> = self.tabs().map(|t| t.repo.clone()).collect();
        let saved = Config::load_file(&self.dirs).and_then(|mut c| {
            c.gui_repos = repos;
            c.save(&self.dirs)
        });
        if let Err(e) = saved {
            self.banner = Some(format!("could not remember the open tabs: {e}"));
        }
    }

    /// The bar along the top: the two things that belong to no tab. The
    /// app's name is already in the menu bar, its icon in the Dock. Where nothing is drawn, it moves the window, as a title bar
    /// does: on macOS it is the title bar, under the window's buttons.
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let rect = egui::Rect::from_min_size(
            ui.cursor().min,
            egui::vec2(ui.available_width(), BAR_HEIGHT),
        );
        title_bar(ui, rect);
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.add_space(crate::chrome::LEFT_INSET);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::chrome::icon_button(ui, "⚙", "Settings").clicked() {
                        self.settings.open = !self.settings.open;
                    }
                    let add = ui
                        .button("+  Repository")
                        .on_hover_text("Open a repository in a new tab");
                    egui::Popup::menu(&add)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| self.add_repo_form(ui));
                });
            },
        );
        ui.allocate_rect(rect, egui::Sense::hover());
    }

    /// A problem worth showing until dismissed, along the bottom.
    fn banner(&mut self, ui: &mut egui::Ui) {
        let Some(b) = self.banner.clone() else {
            return;
        };
        let error = ui.visuals().error_fg_color;
        egui::Panel::bottom("banner")
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(ui.visuals().panel_fill)
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ui, |ui| {
                egui::Frame::new()
                    .fill(error.gamma_multiply(0.12))
                    .stroke(egui::Stroke::new(1.0, error.gamma_multiply(0.5)))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(b).color(error));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button("Dismiss").clicked() {
                                        self.banner = None;
                                    }
                                },
                            );
                        });
                    });
            });
    }

    /// The panel "+ Repository" opens: a path, typed or picked.
    fn add_repo_form(&mut self, ui: &mut egui::Ui) {
        ui.set_width(380.0);
        ui.label(
            egui::RichText::new("Open a repository").font(crate::fonts::strong(ui.ctx(), 14.0)),
        );
        ui.add_space(6.0);
        let field = ui.add(
            egui::TextEdit::singleline(&mut self.new_repo)
                .hint_text("/path/to/a/repository")
                .desired_width(f32::INFINITY)
                .margin(egui::Margin::symmetric(8, 6)),
        );
        if self.new_repo.is_empty() && !field.has_focus() {
            field.request_focus();
        }
        let typed = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.add_space(8.0);
        let mut pick: Option<PathBuf> = None;
        ui.horizontal(|ui| {
            if ui.button("Browse…").clicked() {
                pick = rfd::FileDialog::new().pick_folder();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let open = crate::chrome::primary_button(ui, "Open");
                if open.clicked() || typed {
                    let p = self.new_repo.trim();
                    if !p.is_empty() {
                        pick = Some(PathBuf::from(p));
                    }
                }
            });
        });
        if let Some(p) = pick {
            match self.add_repo(&p) {
                Ok(()) => {
                    self.new_repo.clear();
                    self.banner = None;
                    ui.close();
                }
                Err(e) => self.banner = Some(format!("{}: {e}", p.display())),
            }
        }
    }

    /// `egui_dock`'s tabs, in the theme's colours, without the buttons a
    /// window of repositories has no use for.
    fn dock_style(ui: &egui::Ui, titlebar: bool) -> Style {
        let v = ui.visuals();
        let palette = crate::theme::palette(ui.ctx());
        let bar = palette.map_or(v.faint_bg_color, |p| p.panel_bg);
        let body = palette.map_or(v.panel_fill, |p| p.window_bg);
        let border = palette.map_or(v.widgets.noninteractive.bg_stroke.color, |p| p.border);
        let text = palette.map_or(v.text_color(), |p| p.text);
        let muted = v.weak_text_color();
        let mut s = Style::from_egui(ui.style().as_ref());
        s.dock_area_padding = None;
        s.main_surface_border_stroke = egui::Stroke::NONE;
        s.main_surface_border_rounding = egui::CornerRadius::ZERO;
        // The tabs are part of the top bar: flat, square, on its colour.
        // The open one is told by its text and the accent line under it,
        // drawn in `on_tab_button`.
        s.tab_bar.bg_fill = bar;
        // In the title bar, the tabs are as tall as it is and start past
        // the window's buttons.
        s.tab_bar.height = if titlebar { BAR_HEIGHT + 4.0 } else { 34.0 };
        s.tab_bar.hline_color = border;
        s.tab_bar.corner_radius = egui::CornerRadius::ZERO;
        s.tab_bar.inner_margin = egui::Margin {
            left: if titlebar {
                crate::chrome::LEFT_INSET as i8
            } else {
                6
            },
            right: 6,
            top: 0,
            bottom: 0,
        };
        let flat = |text: egui::Color32, bg: egui::Color32| egui_dock::TabInteractionStyle {
            bg_fill: bg,
            text_color: text,
            outline_color: egui::Color32::TRANSPARENT,
            corner_radius: egui::CornerRadius::ZERO,
        };
        s.tab.active = flat(text, bar);
        s.tab.focused = s.tab.active.clone();
        s.tab.active_with_kb_focus = s.tab.active.clone();
        s.tab.focused_with_kb_focus = s.tab.active.clone();
        s.tab.inactive = flat(muted, bar);
        s.tab.inactive_with_kb_focus = s.tab.inactive.clone();
        s.tab.hovered = flat(text, bar);
        s.tab.hline_below_active_tab_name = true;
        s.tab.spacing = 2.0;
        s.tab.tab_body.bg_fill = body;
        s.tab.tab_body.stroke = egui::Stroke::NONE;
        s.tab.tab_body.inner_margin = egui::Margin::ZERO;
        s.tab.tab_body.corner_radius = egui::CornerRadius::ZERO;
        s.tab.minimum_width = None;
        s.buttons.close_tab_color = muted.gamma_multiply(0.7);
        s.buttons.close_tab_active_color = text;
        s.buttons.close_tab_bg_fill = v.widgets.hovered.weak_bg_fill;
        s
    }
}

/// A title bar's behaviour over `rect`: dragging it moves the window, a
/// double click zooms it. Registered before what is drawn on it, which
/// stays on top.
fn title_bar(ui: &egui::Ui, rect: egui::Rect) {
    let drag = ui.interact(
        rect,
        ui.id().with("titlebar"),
        egui::Sense::click_and_drag(),
    );
    if drag.drag_started() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    if drag.double_clicked() {
        let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
}

/// No repository open yet: say how to add one.
fn welcome(ui: &mut egui::Ui, menu_bar: bool) {
    let muted = ui.visuals().weak_text_color();
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.32);
        crate::chrome::mark(ui, 52.0);
        ui.add_space(16.0);
        ui.label(
            egui::RichText::new("No repository open").font(crate::fonts::strong(ui.ctx(), 20.0)),
        );
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(if menu_bar {
                "Open one with File › Open Repository… (⌘O). It comes back as a tab \
                 every time zrush opens. ⌘K finds everything else."
            } else {
                "Open one with + Repository, top right. It comes back as a tab every \
                 time zrush opens."
            })
            .color(muted),
        );
    });
}

/// Draws one tab; collects what was closed, to be saved after the frame.
struct Viewer<'a> {
    closed: Vec<PathBuf>,
    md: &'a mut egui_commonmark::CommonMarkCache,
    /// The tabs open last frame: their buttons are drawn before their
    /// bodies, so this frame's are not known yet.
    shown: &'a [PathBuf],
    now_shown: Vec<PathBuf>,
    accent: egui::Color32,
    live: egui::Color32,
}

impl TabViewer for Viewer<'_> {
    type Tab = Tab;

    fn id(&mut self, tab: &mut Tab) -> egui::Id {
        egui::Id::new(("tab", &tab.repo))
    }

    /// The repository's name, then how many of its sessions run, in the
    /// running colour: no dot, a glyph from another font sits off the line.
    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        let n = tab.live_count();
        if n == 0 {
            return tab.title.clone().into();
        }
        let mut job = egui::text::LayoutJob::default();
        job.append(
            &tab.title,
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional(13.5),
                color: egui::Color32::PLACEHOLDER,
                valign: egui::Align::Center,
                ..Default::default()
            },
        );
        job.append(
            &n.to_string(),
            7.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional(11.5),
                color: self.live,
                valign: egui::Align::Center,
                ..Default::default()
            },
        );
        job.into()
    }

    /// The panes scroll themselves. The dock's own scroll bars, on by
    /// default, would shift the whole tab sideways for one wide line.
    fn scroll_bars(&self, _tab: &Tab) -> [bool; 2] {
        [false, false]
    }

    /// The open tab's accent line, along its bottom edge.
    fn on_tab_button(&mut self, tab: &mut Tab, response: &egui::Response) {
        if !self.shown.contains(&tab.repo) {
            return;
        }
        let r = response.rect;
        let line = egui::Rect::from_min_max(
            egui::pos2(r.left() + 10.0, r.bottom() - 2.0),
            egui::pos2(r.right() - 10.0, r.bottom()),
        );
        response
            .ctx
            .layer_painter(response.layer_id)
            .rect_filled(line, 1.0, self.accent);
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Tab) {
        self.now_shown.push(tab.repo.clone());
        if let Some(q) = crate::panes::show(ui, tab, self.md) {
            tab.ask(q);
        }
        crate::dialog::show(ui.ctx(), tab);
    }

    fn on_close(&mut self, tab: &mut Tab) -> OnCloseResponse {
        self.closed.push(tab.repo.clone());
        OnCloseResponse::Close
    }
}

impl eframe::App for GuiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if !self.fonts {
            crate::fonts::install(ui.ctx());
            self.fonts = true;
        }
        self.apply_theme(ui.ctx());
        let now = Instant::now();
        for t in self.tabs_mut() {
            t.poll();
            t.refresh_if_due(now);
        }
        let title = self.window_title();
        if title != self.shown_title {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.shown_title = title;
        }

        self.commands(ui.ctx());
        let bar_fill =
            crate::theme::palette(ui.ctx()).map_or(ui.visuals().faint_bg_color, |p| p.panel_bg);
        self.banner(ui);
        let titlebar = self.titlebar_tabs;
        let has_tabs = self.dock.iter_all_tabs().next().is_some();
        if !titlebar {
            egui::Panel::top("bar")
                .show_separator_line(false)
                .frame(
                    egui::Frame::new()
                        .fill(bar_fill)
                        // No margin above or below: the bar is as tall as
                        // macOS's title bar, and its buttons sit on the same
                        // line as the window's.
                        .inner_margin(egui::Margin::symmetric(10, 0)),
                )
                .show(ui, |ui| self.top_bar(ui));
        }
        let (accent, _) = crate::chrome::accent(ui);
        let live = crate::theme::palette(ui.ctx()).map_or(ui.visuals().hyperlink_color, |p| p.live);
        let shown = std::mem::take(&mut self.shown_tabs);
        let mut viewer = Viewer {
            closed: Vec::new(),
            md: &mut self.md,
            shown: &shown,
            now_shown: Vec::new(),
            accent,
            live,
        };
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(ui, |ui| {
                if titlebar {
                    // The strip the window's buttons sit in moves the
                    // window: under the tabs, which stay on top of it.
                    let strip = egui::Rect::from_min_size(
                        ui.max_rect().min,
                        egui::vec2(ui.max_rect().width(), BAR_HEIGHT + 4.0),
                    );
                    title_bar(ui, strip);
                }
                if !has_tabs {
                    if titlebar {
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(
                                ui.max_rect().min,
                                egui::vec2(ui.max_rect().width(), BAR_HEIGHT + 4.0),
                            ),
                            0.0,
                            bar_fill,
                        );
                    }
                    welcome(ui, titlebar);
                    return;
                }
                DockArea::new(&mut self.dock)
                    .style(Self::dock_style(ui, titlebar))
                    .show_leaf_collapse_buttons(false)
                    .show_leaf_close_all_buttons(false)
                    .show_inside(ui, &mut viewer);
            });
        let closed = !viewer.closed.is_empty();
        let now_shown = std::mem::take(&mut viewer.now_shown);
        drop(viewer);
        self.shown_tabs = now_shown;
        if closed {
            self.save_repos();
        }
        if self.settings.open {
            if let Some(z) = self.settings_service() {
                if let Some(crate::settings::Changed::Config(cfg)) =
                    crate::settings::show(ui.ctx(), &mut self.settings, &z, &self.themes)
                {
                    self.apply_settings(&cfg);
                }
            } else {
                self.settings.open = false;
                self.banner = Some("Open a repository to change the settings".into());
            }
        }
        self.show_palette(ui.ctx());
        // Idle tabs still ask again now and then.
        ui.ctx().request_repaint_after(REFRESH_EVERY);
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use zrush_core::config::{Config, dirs_under};
    use zrush_core::host::RecordingHost;

    use super::*;
    use crate::testing::{git, scratch_repo};

    fn app(td: &tempfile::TempDir, repos: Vec<PathBuf>) -> GuiApp {
        GuiApp::new(
            dirs_under(td.path()),
            Config::default(),
            repos,
            Box::new(|_| Box::new(RecordingHost::default())),
            Arc::new(|| {}),
        )
    }

    /// A second repository beside the first scratch one.
    fn other_repo(td: &tempfile::TempDir) -> PathBuf {
        let base = zrush_core::paths::real(td.path()).join("second");
        std::fs::create_dir_all(&base).unwrap();
        git(&base, &["init", "-q"]);
        git(&base, &["config", "user.email", "t@example.invalid"]);
        git(&base, &["config", "user.name", "zrush tests"]);
        std::fs::write(base.join("g.txt"), "x").unwrap();
        git(&base, &["add", "g.txt"]);
        git(&base, &["commit", "-qm", "init"]);
        base
    }

    #[test]
    fn a_repo_added_twice_opens_once() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        git(
            &repo,
            &["worktree", "add", "-q", ".claude/worktrees/x", "-b", "x"],
        );
        let mut a = app(&td, Vec::new());
        a.add_repo(&repo).unwrap();
        a.add_repo(&repo.join(".claude/worktrees/x")).unwrap();
        assert_eq!(a.tab_count(), 1);
    }

    #[test]
    fn a_path_that_is_not_a_repo_is_refused_without_a_tab() {
        let td = tempfile::TempDir::new().unwrap();
        let mut a = app(&td, Vec::new());
        assert!(a.add_repo(td.path()).is_err());
        assert_eq!(a.tab_count(), 0);
    }

    #[test]
    fn opening_and_closing_tabs_is_remembered() {
        let td = tempfile::TempDir::new().unwrap();
        let first = scratch_repo(&td);
        let second = other_repo(&td);
        let mut a = app(&td, Vec::new());
        a.add_repo(&first).unwrap();
        a.add_repo(&second).unwrap();
        a.close_tab_for(&first);
        let saved = Config::load_file(&dirs_under(td.path())).unwrap();
        assert_eq!(saved.gui_repos, vec![second]);
    }

    #[test]
    fn switching_agent_reaches_every_tab() {
        // What every tab listed belonged to the agent just left.
        let td = tempfile::TempDir::new().unwrap();
        let first = scratch_repo(&td);
        let second = other_repo(&td);
        let mut a = app(&td, vec![first.clone(), second]);
        let live = zrush_core::model::Session {
            agent: "claude",
            id: "x".into(),
            title: "t".into(),
            status: "busy 0m".into(),
            cwd: first.clone(),
            launch_cwd: first,
            kind: zrush_core::model::SessionKind::Live,
            last_activity: 0,
        };
        a.tabs_mut().for_each(|t| t.snap.live = vec![live.clone()]);
        let cfg = Config {
            agent: Some("claude".into()),
            ..Config::default()
        };
        a.apply_settings(&cfg);
        for t in a.tabs() {
            assert_eq!(
                t.snap.live.len(),
                0,
                "{} kept the old agent's sessions",
                t.title
            );
        }
    }

    #[test]
    fn the_title_counts_live_sessions() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let mut a = app(&td, vec![repo.clone()]);
        assert_eq!(a.window_title(), "zrush");
        let live = zrush_core::model::Session {
            agent: "claude",
            id: "x".into(),
            title: "t".into(),
            status: "busy 0m".into(),
            cwd: repo.clone(),
            launch_cwd: repo,
            kind: zrush_core::model::SessionKind::Live,
            last_activity: 0,
        };
        let elsewhere = zrush_core::model::Session {
            cwd: "/somewhere/else".into(),
            launch_cwd: "/somewhere/else".into(),
            ..live.clone()
        };
        a.tabs_mut().for_each(|t| {
            t.snap.live = vec![live.clone(); 3];
            t.snap.live.push(elsewhere.clone());
        });
        // The one running in another repository is not this window's.
        assert_eq!(a.window_title(), "zrush — ● 3 live");
    }

    #[test]
    fn a_saved_repo_that_is_gone_shows_its_error_and_the_others_load() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let gone = td.path().join("gone");
        let a = app(&td, vec![gone.clone(), repo]);
        let error = a
            .tabs()
            .find(|t| t.repo == gone)
            .and_then(|t| t.error.clone())
            .unwrap();
        let mut h = Harness::builder()
            .with_size([1000.0, 700.0])
            .build_eframe(|_cc| a);
        // Both tabs are in the bar; the gone one's body says why.
        let deadline = Instant::now() + Duration::from_secs(10);
        while h.query_all_by_label_contains("main").next().is_none() && Instant::now() < deadline {
            h.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        // The good tab loaded (its main worktree is listed) and the gone one
        // is there too; showing it shows why it is empty.
        assert!(h.query_all_by_label_contains("main").next().is_some());
        assert_eq!(h.state().tab_count(), 2);
        h.state_mut().focus(&gone);
        h.run_steps(3);
        assert!(h.query_by_label_contains(&error).is_some(), "{error}");
    }

    fn harness<'a>(a: GuiApp) -> Harness<'a, GuiApp> {
        Harness::builder()
            .with_size([1000.0, 700.0])
            .build_eframe(|_cc| a)
    }

    fn settle(h: &mut Harness<'_, GuiApp>, done: impl Fn(&Harness<'_, GuiApp>) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done(h) && Instant::now() < deadline {
            h.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        h.run_steps(2);
    }

    #[test]
    fn command_k_finds_a_session_and_shows_it() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        crate::testing::with_session(&td, &repo, &["hello", "hi there"]);
        let mut h = harness(app(&td, vec![repo]));
        settle(&mut h, |h| {
            h.state().tabs().any(|t| {
                t.rows()
                    .iter()
                    .any(|r| r.search == crate::testing::SESSION_TITLE)
            })
        });
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::K);
        h.run_steps(2);
        assert!(h.state().palette.open, "⌘K did not open the palette");
        h.event(egui::Event::Text("port rust".into()));
        h.run_steps(2);
        h.key_press(egui::Key::Enter);
        h.run_steps(2);
        assert!(!h.state().palette.open, "Enter left it open");
        let selected = h.state().tabs().next().and_then(|t| t.selected.clone());
        assert_eq!(
            selected,
            Some(format!("s:{}", crate::testing::SESSION_ID)),
            "the session was not shown"
        );
    }

    #[test]
    fn the_palette_runs_the_apps_commands_and_escape_closes_it() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let mut h = harness(app(&td, vec![repo]));
        settle(&mut h, |h| h.state().tabs().any(|t| !t.rows().is_empty()));
        h.state_mut().open_palette();
        h.run_steps(2);
        h.key_press(egui::Key::Escape);
        h.run_steps(2);
        assert!(!h.state().palette.open);
        h.state_mut().open_palette();
        h.run_steps(2);
        h.event(egui::Event::Text("settings".into()));
        h.run_steps(2);
        h.key_press(egui::Key::Enter);
        h.run_steps(2);
        assert!(h.state().settings.open, "Settings was not opened");
    }
}
