//! The window: one tab per repository, and the bar to open more.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use egui_dock::tab_viewer::OnCloseResponse;
use egui_dock::{DockArea, DockState, Style, TabViewer};
use zrush_core::config::{Config, Dirs};
use zrush_core::host::Host;

use crate::tab::{REFRESH_EVERY, Tab, Wake};

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
    md: egui_commonmark::CommonMarkCache,
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
            md: egui_commonmark::CommonMarkCache::default(),
        };
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

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Repository");
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.new_repo)
                    .hint_text("path to a repository")
                    .desired_width(360.0),
            );
            let typed = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let mut pick: Option<PathBuf> = None;
            if ui.button("Open").clicked() || typed {
                let p = self.new_repo.trim();
                if !p.is_empty() {
                    pick = Some(PathBuf::from(p));
                }
            }
            if ui.button("Browse…").clicked() {
                pick = rfd::FileDialog::new().pick_folder();
            }
            if let Some(p) = pick {
                match self.add_repo(&p) {
                    Ok(()) => {
                        self.new_repo.clear();
                        self.banner = None;
                    }
                    Err(e) => self.banner = Some(format!("{}: {e}", p.display())),
                }
            }
        });
        if let Some(b) = self.banner.clone() {
            ui.horizontal(|ui| {
                ui.colored_label(ui.visuals().error_fg_color, b);
                if ui.small_button("✕").clicked() {
                    self.banner = None;
                }
            });
        }
    }
}

/// Draws one tab; collects what was closed, to be saved after the frame.
struct Viewer<'a> {
    closed: Vec<PathBuf>,
    md: &'a mut egui_commonmark::CommonMarkCache,
}

impl TabViewer for Viewer<'_> {
    type Tab = Tab;

    fn id(&mut self, tab: &mut Tab) -> egui::Id {
        egui::Id::new(("tab", &tab.repo))
    }

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        match tab.live_count() {
            0 => tab.title.clone().into(),
            n => format!("{}  ● {n}", tab.title).into(),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Tab) {
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

        egui::Panel::top("bar").show(ui, |ui| self.top_bar(ui));
        let mut viewer = Viewer {
            closed: Vec::new(),
            md: &mut self.md,
        };
        egui::CentralPanel::default().show(ui, |ui| {
            DockArea::new(&mut self.dock)
                .style(Style::from_egui(ui.style().as_ref()))
                .show_inside(ui, &mut viewer);
        });
        if !viewer.closed.is_empty() {
            self.save_repos();
        }
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
        a.tabs_mut()
            .for_each(|t| t.snap.live = vec![live.clone(); 3]);
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
        while h.query_by_label_contains("main").is_none() && Instant::now() < deadline {
            h.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        // The good tab loaded (its main worktree is listed) and the gone one
        // is there too; showing it shows why it is empty.
        assert!(h.query_by_label_contains("main").is_some());
        assert_eq!(h.state().tab_count(), 2);
        h.state_mut().focus(&gone);
        h.run_steps(3);
        assert!(h.query_by_label_contains(&error).is_some(), "{error}");
    }
}
