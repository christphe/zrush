//! One repository in a tab: the core's service, loader and snapshot for it,
//! and what the tab is showing. No egui here, so all of it is testable
//! without a window.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use zrush_core::app::Zrush;
use zrush_core::config::{Config, Dirs};
use zrush_core::host::Host;
use zrush_core::loader::{Event, Loader, Preview, Sink};
use zrush_core::model::tree::Row;
use zrush_core::snapshot::{Applied, RepoSnapshot, TreeView, preview_key};

/// Called after every loader result, from the loader's threads: the window
/// passes its repaint request, so a result shows without waiting for input.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// How often an idle tab asks again, so sessions starting or ending
/// elsewhere show up without a click.
pub const REFRESH_EVERY: Duration = Duration::from_secs(10);

pub struct Tab {
    /// The path the tab was opened with, as saved in `gui_repos`.
    pub repo: PathBuf,
    /// The main worktree's folder name; the path's last part when broken.
    pub title: String,
    pub snap: RepoSnapshot,
    pub view: TreeView,
    /// The preview key of the row being shown on the right.
    pub selected: Option<String>,
    /// Something went wrong; shown until dismissed.
    pub error: Option<String>,
    /// A line worth saying, not an error.
    pub flash: Option<String>,
    /// What a long action is doing, while it runs.
    pub busy: Option<String>,
    live: Option<Live>,
    rows: Vec<Row>,
    last_refresh: Instant,
}

/// What a tab has when its repository could be opened.
struct Live {
    z: Arc<Zrush>,
    loader: Loader,
    rx: Receiver<Event>,
}

impl Tab {
    /// Never fails: a repository that cannot be opened is a tab showing
    /// why, so the other tabs still open and this one can be closed.
    pub fn open(repo: &Path, dirs: &Dirs, cfg: &Config, host: Box<dyn Host>, wake: Wake) -> Self {
        let fallback = repo.file_name().map_or_else(
            || repo.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let mut tab = Self {
            repo: repo.to_path_buf(),
            title: fallback,
            snap: RepoSnapshot::default(),
            view: TreeView {
                resumable_max: cfg.resumable_max,
                ..TreeView::default()
            },
            selected: None,
            error: None,
            flash: None,
            busy: None,
            live: None,
            rows: Vec::new(),
            last_refresh: Instant::now(),
        };
        match Zrush::for_repo(dirs, cfg, repo.to_path_buf(), host, false, None) {
            Ok(z) => {
                // The whole conversation, for this window only.
                z.keep_override(|c| c.preview_turns = usize::MAX);
                if let Some(name) = z.main_root().file_name() {
                    tab.title = name.to_string_lossy().into_owned();
                }
                let (tx, rx) = channel();
                let sink: Sink = Arc::new(move |e| {
                    let listening = tx.send(e).is_ok();
                    wake();
                    listening
                });
                tab.live = Some(Live {
                    z: Arc::new(z),
                    loader: Loader::new(sink),
                    rx,
                });
                tab.refresh();
            }
            Err(e) => tab.error = Some(e.to_string()),
        }
        tab
    }

    pub fn zrush(&self) -> Option<&Arc<Zrush>> {
        self.live.as_ref().map(|l| &l.z)
    }

    /// `None` for a tab whose repository could not be opened.
    pub fn main_root(&self) -> Option<&Path> {
        self.zrush().map(|z| z.main_root())
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn live_count(&self) -> usize {
        self.snap.live.len()
    }

    pub fn last_refresh(&self) -> Instant {
        self.last_refresh
    }

    pub fn refresh(&mut self) {
        self.last_refresh = Instant::now();
        if let Some(l) = &self.live {
            l.loader.refresh(&l.z, self.view.show_all);
        }
    }

    pub fn refresh_if_due(&mut self, now: Instant) {
        if now.duration_since(self.last_refresh) >= REFRESH_EVERY {
            self.refresh();
        }
    }

    /// Take whatever the loader has sent. Returns whether anything changed.
    pub fn poll(&mut self) -> bool {
        let Some(l) = &self.live else {
            return false;
        };
        let mut changed = false;
        let mut rows_stale = false;
        let mut again = false;
        while let Ok(event) = l.rx.try_recv() {
            match self.snap.apply(event, l.loader.generation()) {
                Applied::Stale | Applied::Nothing | Applied::Update(_) => continue,
                Applied::Rows => rows_stale = true,
                Applied::Preview => {}
                Applied::Acted(done) => {
                    self.busy = None;
                    match done.result {
                        Ok(msg) => self.flash = Some(msg),
                        Err(e) => self.error = Some(format!("{}: {e}", done.label)),
                    }
                    again = true;
                }
                Applied::Failed(e) => self.flash = Some(e),
            }
            changed = true;
        }
        if rows_stale {
            self.rows = self.snap.rows(&l.z, &self.view);
        }
        if again {
            self.refresh();
        }
        changed
    }

    /// Show this row on the right, fetching its preview once.
    pub fn select(&mut self, row: &Row) {
        self.selected = preview_key(row);
        let Some(l) = &self.live else {
            return;
        };
        let listed = l.z.active_agent().map_or("", zrush_core::agent::Agent::id);
        if let Some((key, job)) = self.snap.want_preview(row, listed) {
            l.loader.request_preview(&l.z, key, job);
        }
    }

    pub fn preview(&self) -> Preview {
        self.selected
            .as_ref()
            .map_or(Preview::Empty, |k| self.snap.preview(k))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use zrush_core::config::{Config, dirs_under};
    use zrush_core::host::RecordingHost;
    use zrush_core::loader::Preview;
    use zrush_core::model::tree::RowKind;

    use super::*;
    use crate::testing::scratch_repo;

    fn open(td: &tempfile::TempDir, repo: &Path) -> Tab {
        Tab::open(
            repo,
            &dirs_under(td.path()),
            &Config::default(),
            Box::new(RecordingHost::default()),
            Arc::new(|| {}),
        )
    }

    /// Poll until `done` says so, or give up after 10 s.
    fn until(t: &mut Tab, done: impl Fn(&Tab) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done(t) && Instant::now() < deadline {
            t.poll();
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn a_tab_loads_its_worktrees() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let mut t = open(&td, &repo);
        until(&mut t, |t| !t.rows().is_empty());
        assert_eq!(t.rows()[0].kind, RowKind::Worktree);
        assert!(t.error.is_none());
    }

    #[test]
    fn a_repo_that_is_gone_is_a_tab_with_its_error() {
        let td = tempfile::TempDir::new().unwrap();
        let t = open(&td, &td.path().join("gone"));
        assert!(t.error.is_some());
        assert_eq!(t.title, "gone");
        assert_eq!(t.rows().len(), 0);
        assert!(t.main_root().is_none());
    }

    #[test]
    fn the_title_is_the_main_worktrees_folder() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        assert_eq!(open(&td, &repo).title, "repo");
    }

    #[test]
    fn selecting_a_worktree_fetches_its_git_preview() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let mut t = open(&td, &repo);
        until(&mut t, |t| !t.rows().is_empty());
        let row = t.rows()[0].clone();
        t.select(&row);
        until(&mut t, |t| t.preview() != Preview::Empty);
        assert!(
            matches!(t.preview(), Preview::Worktree(l) if l.iter().any(|x| x.contains("init")))
        );
    }

    #[test]
    fn dropping_a_tab_mid_load_does_not_panic() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let t = open(&td, &repo);
        drop(t);
        std::thread::sleep(Duration::from_millis(500));
    }

    #[test]
    fn the_gui_reads_whole_conversations_without_saving_it() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let t = open(&td, &repo);
        assert_eq!(t.zrush().unwrap().config().preview_turns, usize::MAX);
        assert!(!dirs_under(td.path()).config_file().exists());
    }

    #[test]
    fn a_refresh_is_due_after_the_interval() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let mut t = open(&td, &repo);
        let started = t.last_refresh();
        t.refresh_if_due(started + REFRESH_EVERY / 2);
        assert_eq!(t.last_refresh(), started);
        t.refresh_if_due(started + REFRESH_EVERY);
        assert!(t.last_refresh() > started);
    }
}
