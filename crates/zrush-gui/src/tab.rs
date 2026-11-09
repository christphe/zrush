//! One repository in a tab: the core's service, loader and snapshot for it,
//! and what the tab is showing. No egui here, so all of it is testable
//! without a window.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use zrush_core::actions;
use zrush_core::app::BranchKind;
use zrush_core::app::Zrush;
use zrush_core::config::{Config, Dirs};
use zrush_core::host::Host;
use zrush_core::intent::{self, Answer, Intent, Outcome, Question};
use zrush_core::loader::{Event, Loader, Preview, Sink};
use zrush_core::model::SessionKind;
use zrush_core::model::tree::{NodeId, Row, RowKind};
use zrush_core::purge::{self, Plan};
use zrush_core::snapshot::{Applied, RepoSnapshot, TreeView, preview_key};

/// Called after every loader result, from the loader's threads: the window
/// passes its repaint request, so a result shows without waiting for input.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// How often an idle tab asks again, so sessions starting or ending
/// elsewhere show up without a click.
pub const REFRESH_EVERY: Duration = Duration::from_secs(10);

/// A question the tab has put up, waiting for an answer.
#[derive(Debug, Clone)]
pub enum Pending {
    Ask(Question),
    /// A new worktree needs a branch: which kind first, then its name.
    /// `at` is where it lands, `hand_over` the session it gets, if any.
    Branch {
        at: Option<PathBuf>,
        hand_over: Option<(String, String)>,
        kind: Option<BranchKind>,
        name: String,
    },
    /// Something is about to go for good.
    Confirm(Confirm),
}

/// What a confirmation is about. Its answers come safe one last, and
/// every one before it destroys something.
#[derive(Debug, Clone)]
pub enum Confirm {
    DeleteSession {
        agent: String,
        id: String,
        /// The worktree it is listed under, whose association goes too.
        worktree: Option<PathBuf>,
        title: String,
    },
    RemoveWorktree {
        path: PathBuf,
        name: String,
        /// Conversations held in it, which the second answer deletes.
        sessions: usize,
    },
    Purge(Plan),
}

impl Confirm {
    pub fn title(&self) -> &'static str {
        match self {
            Self::DeleteSession { .. } => "Delete session",
            Self::RemoveWorktree { .. } => "Remove worktree",
            Self::Purge(_) => "Purge",
        }
    }

    pub fn body(&self) -> String {
        match self {
            Self::DeleteSession { title, .. } => {
                format!("{title}\n\nIts transcript goes for good.")
            }
            Self::RemoveWorktree { name, sessions, .. } => match sessions {
                0 => format!("{name} has no sessions."),
                n => format!("{name} has {n} session(s)."),
            },
            Self::Purge(plan) => plan.summary(),
        }
    }

    pub fn choices(&self) -> Vec<&'static str> {
        match self {
            Self::DeleteSession { .. } => vec!["Delete it", CANCEL],
            Self::RemoveWorktree { sessions: 0, .. } => vec!["Remove it", CANCEL],
            Self::RemoveWorktree { .. } => vec![
                "Remove the worktree, keep the sessions",
                "Remove it and delete the sessions too",
                CANCEL,
            ],
            Self::Purge(_) => vec![
                "Purge, and their other sessions too",
                "Purge only what is marked",
                CANCEL,
            ],
        }
    }
}

const CANCEL: &str = "Cancel";

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
    /// The question that is up, if any.
    pub pending: Option<Pending>,
    /// What the tree is narrowed to, in core's filter syntax.
    pub filter: String,
    /// Purge mode: the rows ticked so far, by preview key, so a refresh
    /// that reorders the rows keeps them.
    pub purging: Option<HashSet<String>>,
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
        let made = Zrush::for_repo(dirs, cfg, repo.to_path_buf(), host, false, None);
        Self::over(repo, cfg, made, wake)
    }

    /// The same, over a zrush already made, or the reason it could not be:
    /// tests make theirs with every agent, installed on the machine or not.
    pub(crate) fn over(
        repo: &Path,
        cfg: &Config,
        made: zrush_core::error::Result<Zrush>,
        wake: Wake,
    ) -> Self {
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
            pending: None,
            filter: String::new(),
            purging: None,
            live: None,
            rows: Vec::new(),
            last_refresh: Instant::now(),
        };
        match made {
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
                        Ok(msg) if msg.is_empty() => {}
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
            // A new round forgot every preview: what is on the right is
            // fetched again (once — the snapshot knows what is on its way).
            if let Some(row) = self.selected_row().cloned() {
                self.select(&row);
            }
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

    /// What enter does on this row: open now, unfold more, or a question
    /// for the window to put up.
    pub fn enter(&mut self, row: &Row) -> Option<Question> {
        let l = self.live.as_ref()?;
        match intent::enter(row, &self.snap) {
            Intent::Nothing => None,
            Intent::Ask(q) => Some(q),
            Intent::Open { worktree, bind } => {
                self.launch("Open", move |z| {
                    let bind = bind.as_ref().map(|(a, i)| (a.as_str(), i.as_str()));
                    z.open(&worktree, bind)
                });
                None
            }
            Intent::ShowMore(node) => {
                let step = l.z.config().more_step;
                let cur = self
                    .view
                    .expanded
                    .get(&node)
                    .copied()
                    .unwrap_or(self.view.resumable_max);
                self.view.expanded.insert(node, cur + step);
                self.last_refresh = Instant::now();
                l.loader.refresh(&l.z, true);
                None
            }
        }
    }

    /// The agent in a terminal of its own: resuming this row's session, or
    /// a fresh one in this row's worktree.
    pub fn run_agent(&mut self, row: &Row) {
        let Some(l) = &self.live else {
            return;
        };
        let NodeId::Worktree(worktree) = row.node.clone() else {
            return;
        };
        let session = row
            .session_id
            .as_deref()
            .and_then(|id| self.snap.session(id))
            .map(|s| (s.agent.to_string(), s.id.clone()));
        if session.is_none() && l.z.active_agent().is_none() {
            self.flash = Some("no agent installed".into());
            return;
        }
        self.launch("New session", move |z| {
            let session = session.as_ref().map(|(a, i)| (a.as_str(), i.as_str()));
            actions::run_agent(z, &worktree, session)
        });
    }

    /// Start an editor or a terminal off the window's thread: one that
    /// stays in the foreground (`zed --wait`) must not hold the window.
    /// Unlike a long action it takes no turn — nothing else waits on it.
    fn launch<F>(&self, label: &str, f: F)
    where
        F: FnOnce(&Zrush) -> zrush_core::error::Result<()> + Send + 'static,
    {
        if let Some(l) = &self.live {
            let z = Arc::clone(&l.z);
            l.loader
                .spawn_action(label, move || f(&z).map(|()| String::new()));
        }
    }

    pub fn ask(&mut self, q: Question) {
        self.pending = Some(Pending::Ask(q));
    }

    /// Carry out the answer to the question that is up.
    pub fn answer(&mut self, a: Answer) {
        let Some(Pending::Ask(q)) = self.pending.clone() else {
            self.pending = None;
            return;
        };
        match intent::answer(&q, a, &self.snap) {
            Outcome::Nothing => self.pending = None,
            Outcome::Open { worktree, bind } => {
                self.pending = None;
                self.launch("Open", move |z| {
                    let bind = bind.as_ref().map(|(a, i)| (a.as_str(), i.as_str()));
                    z.open(&worktree, bind)
                });
            }
            Outcome::Adopt {
                agent,
                id,
                worktree,
            } => {
                if self.start("Resume this session", move |z| {
                    actions::adopt(z, &agent, &id, &worktree)
                }) {
                    self.pending = None;
                }
            }
            Outcome::NeedBranch { at, hand_over } => {
                self.pending = Some(Pending::Branch {
                    at,
                    hand_over: Some(hand_over),
                    kind: None,
                    name: String::new(),
                });
            }
        }
    }

    /// Create the worktree the branch question was for, and hand it the
    /// session. An empty name does nothing.
    pub fn create_branch(&mut self) {
        let Some(Pending::Branch {
            at,
            hand_over,
            kind: Some(kind),
            name,
        }) = self.pending.clone()
        else {
            return;
        };
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        if self.start("New worktree", move |z| {
            let bind = hand_over.as_ref().map(|(a, i)| (a.as_str(), i.as_str()));
            actions::create(z, &name, kind, at.as_deref(), bind)
        }) {
            self.pending = None;
        }
    }

    /// Ask for a new worktree. From a session's row, the session moves to
    /// it; from anywhere else it starts fresh.
    pub fn new_worktree(&mut self, from: Option<&Row>) {
        let hand_over = from
            .and_then(|r| r.session_id.as_deref())
            .and_then(|id| self.snap.session(id))
            .map(|s| (s.agent.to_string(), s.id.clone()));
        self.pending = Some(Pending::Branch {
            at: None,
            hand_over,
            kind: None,
            name: String::new(),
        });
    }

    /// Ask before deleting this row's session or removing its worktree. A
    /// running session and the main worktree are refused up front.
    pub fn ask_delete(&mut self, row: &Row) {
        match (row.kind, &row.node) {
            (RowKind::Session | RowKind::Orphan, node) => {
                let Some(s) = row
                    .session_id
                    .as_deref()
                    .and_then(|id| self.snap.session(id))
                else {
                    return;
                };
                if s.kind == SessionKind::Live {
                    self.flash = Some("that session is running; stop it first".into());
                    return;
                }
                self.pending = Some(Pending::Confirm(Confirm::DeleteSession {
                    agent: s.agent.to_string(),
                    id: s.id.clone(),
                    worktree: match node {
                        NodeId::Worktree(p) => Some(p.clone()),
                        NodeId::Orphans => None,
                    },
                    title: row.search.clone(),
                }));
            }
            (RowKind::Worktree, NodeId::Worktree(path)) => {
                if row.facts.root {
                    self.flash = Some("the main worktree stays: git will not remove it".into());
                    return;
                }
                self.pending = Some(Pending::Confirm(Confirm::RemoveWorktree {
                    path: path.clone(),
                    name: path.file_name().map_or_else(
                        || path.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    ),
                    sessions: self.snap.sessions_in(path).len(),
                }));
            }
            _ => {}
        }
    }

    /// Carry out answer `choice` to the confirmation that is up. The last
    /// answer is always Cancel.
    pub fn confirm(&mut self, choice: usize) {
        let Some(Pending::Confirm(c)) = self.pending.clone() else {
            return;
        };
        if choice + 1 >= c.choices().len() {
            self.pending = None;
            return;
        }
        let started = match c {
            Confirm::DeleteSession {
                agent,
                id,
                worktree,
                ..
            } => self.start("Delete session", move |z| {
                actions::delete_session(z, &agent, &id, worktree.as_deref())
            }),
            Confirm::RemoveWorktree { path, sessions, .. } => {
                let with_sessions = sessions > 0 && choice == 1;
                let held = self.snap.sessions_in(&path);
                self.start("Remove worktree", move |z| {
                    actions::remove(z, &path, &held, with_sessions)
                })
            }
            Confirm::Purge(plan) => {
                let plan = if choice == 0 {
                    plan.with_others(&self.snap)
                } else {
                    plan
                };
                let started = self.start("Purge", move |z| actions::purge(z, &plan));
                if started {
                    self.leave_purge();
                }
                started
            }
        };
        if started {
            self.pending = None;
        }
    }

    /// Purge mode: every session listed, nothing ticked yet.
    pub fn start_purge(&mut self) {
        self.purging = Some(HashSet::new());
        self.view.show_all = true;
        self.refresh();
    }

    pub fn leave_purge(&mut self) {
        self.purging = None;
        self.view.show_all = false;
        self.refresh();
    }

    pub fn toggle_mark(&mut self, row: &Row) {
        if let (Some(marked), Some(key)) = (&mut self.purging, preview_key(row))
            && !marked.remove(&key)
        {
            marked.insert(key);
        }
    }

    pub fn is_marked(&self, row: &Row) -> bool {
        self.purging
            .as_ref()
            .zip(preview_key(row))
            .is_some_and(|(m, k)| m.contains(&k))
    }

    /// Ask before purging what is ticked, by core's rules.
    pub fn ask_purge(&mut self) {
        let Some(marked) = &self.purging else {
            return;
        };
        let at = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| preview_key(r).is_some_and(|k| marked.contains(&k)))
            .map(|(i, _)| i);
        let plan = purge::plan(&self.rows, at, &self.snap);
        if plan.is_empty() {
            self.flash = Some("nothing to purge: tick rows first".into());
            return;
        }
        self.pending = Some(Pending::Confirm(Confirm::Purge(plan)));
    }

    /// Run a long action off the window's thread. A second one is refused
    /// while one runs: two `git worktree` commands at once on the same
    /// repository is not something to find out about afterwards.
    fn start<F>(&mut self, label: &str, f: F) -> bool
    where
        F: FnOnce(&Zrush) -> zrush_core::error::Result<String> + Send + 'static,
    {
        if let Some(busy) = &self.busy {
            self.flash = Some(format!("still {busy}"));
            return false;
        }
        let Some(l) = &self.live else {
            return false;
        };
        let z = Arc::clone(&l.z);
        self.busy = Some(label.to_lowercase());
        l.loader.spawn_action(label, move || f(&z));
        true
    }

    /// The row currently shown on the right, if it is still listed.
    pub fn selected_row(&self) -> Option<&Row> {
        let key = self.selected.as_ref()?;
        self.rows
            .iter()
            .find(|r| preview_key(r).as_ref() == Some(key))
    }

    pub fn preview(&self) -> Preview {
        self.selected
            .as_ref()
            .map_or(Preview::Empty, |k| self.snap.preview(k))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use zrush_core::config::dirs_under;
    use zrush_core::host::RecordingHost;
    use zrush_core::loader::Preview;
    use zrush_core::model::tree::RowKind;

    use zrush_core::app::BranchKind;
    use zrush_core::intent::Answer;

    use super::*;
    use crate::testing::scratch_repo;

    fn open(td: &tempfile::TempDir, repo: &Path) -> Tab {
        crate::testing::open_tab(td, repo, Box::new(RecordingHost::default()))
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

    /// An editor that stays in the foreground: `zed --wait`, `subl -w`.
    struct Slow;

    impl zrush_core::host::Host for Slow {
        fn open_workspace(&self, _: &Path) -> zrush_core::error::Result<()> {
            std::thread::sleep(Duration::from_secs(2));
            Ok(())
        }
        fn run_agent(&self, _: &Path, _: &[String]) -> zrush_core::error::Result<()> {
            std::thread::sleep(Duration::from_secs(2));
            Ok(())
        }
    }

    #[test]
    fn opening_the_editor_never_holds_the_window() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let mut t = crate::testing::open_tab(&td, &repo, Box::new(Slow));
        until(&mut t, |t| !t.rows().is_empty());
        let row = t.rows()[0].clone();
        let started = Instant::now();
        let _ = t.enter(&row);
        t.run_agent(&row);
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "the window waited {:?} for the editor",
            started.elapsed()
        );
    }

    #[test]
    fn the_preview_survives_a_refresh() {
        // Every round starts by forgetting the previews; what is selected
        // must be fetched again rather than left as "Reading…".
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let mut t = open(&td, &repo);
        until(&mut t, |t| !t.rows().is_empty());
        let row = t.rows()[0].clone();
        t.select(&row);
        until(&mut t, |t| t.preview() != Preview::Empty);
        t.refresh();
        // Let the new round land, then give the preview time to come back.
        until(&mut t, |t| t.preview() == Preview::Empty);
        until(&mut t, |t| t.preview() != Preview::Empty);
        assert_ne!(t.preview(), Preview::Empty, "preview gone after refresh");
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

    /// A repo whose session was held in a worktree that has since been
    /// removed: it lists as an orphan.
    pub(crate) fn loaded_with_orphan(td: &tempfile::TempDir) -> (Tab, PathBuf) {
        let repo = scratch_repo(td);
        let gone = repo.join(".claude").join("worktrees").join("gone");
        crate::testing::with_session(td, &gone, &["hi", "hello"]);
        let mut t = open(td, &repo);
        until(&mut t, |t| {
            t.rows().iter().any(|r| r.kind == RowKind::Orphan)
        });
        (t, gone)
    }

    /// Enter on the orphan row, as a click would.
    pub(crate) fn orphan_question(t: &mut Tab) -> zrush_core::intent::Question {
        let row = t
            .rows()
            .iter()
            .find(|r| r.kind == RowKind::Orphan)
            .cloned()
            .unwrap();
        t.enter(&row).expect("an orphan asks where")
    }

    fn until_idle(t: &mut Tab) {
        until(t, |t| t.busy.is_none());
    }

    #[test]
    fn moving_an_orphan_to_main_adopts_it() {
        let td = tempfile::TempDir::new().unwrap();
        let (mut t, _) = loaded_with_orphan(&td);
        let q = orphan_question(&mut t);
        t.ask(q);
        t.answer(Answer::MoveToMain);
        assert!(t.busy.is_some());
        until_idle(&mut t);
        let main = t.main_root().unwrap().to_path_buf();
        let bound = t.zrush().unwrap().binding(&main).unwrap();
        assert_eq!(bound.id, crate::testing::SESSION_ID);
        assert!(t.pending.is_none());
    }

    #[test]
    fn recreating_asks_for_a_branch_at_the_old_path() {
        let td = tempfile::TempDir::new().unwrap();
        let (mut t, gone) = loaded_with_orphan(&td);
        let q = orphan_question(&mut t);
        t.ask(q);
        t.answer(Answer::Recreate);
        assert!(
            matches!(&t.pending, Some(Pending::Branch { at: Some(p), .. }) if *p == gone),
            "{:?}",
            t.pending
        );
    }

    #[test]
    fn an_invalid_branch_name_shows_gits_refusal() {
        let td = tempfile::TempDir::new().unwrap();
        let (mut t, gone) = loaded_with_orphan(&td);
        t.pending = Some(Pending::Branch {
            at: Some(gone),
            hand_over: Some(("claude".into(), crate::testing::SESSION_ID.into())),
            kind: Some(BranchKind::New),
            name: "has space".into(),
        });
        t.create_branch();
        until_idle(&mut t);
        let e = t.error.clone().unwrap_or_default();
        assert!(e.contains("invalid branch name"), "{e}");
    }

    #[test]
    fn an_empty_branch_name_creates_nothing() {
        let td = tempfile::TempDir::new().unwrap();
        let (mut t, gone) = loaded_with_orphan(&td);
        t.pending = Some(Pending::Branch {
            at: Some(gone),
            hand_over: Some(("claude".into(), crate::testing::SESSION_ID.into())),
            kind: Some(BranchKind::New),
            name: "  ".into(),
        });
        t.create_branch();
        assert!(t.busy.is_none());
    }

    #[test]
    fn a_second_action_while_one_runs_is_refused() {
        let td = tempfile::TempDir::new().unwrap();
        let (mut t, _) = loaded_with_orphan(&td);
        let q = orphan_question(&mut t);
        t.ask(q);
        t.busy = Some("purge".into());
        t.answer(Answer::MoveToMain);
        assert_eq!(t.flash.as_deref(), Some("still purge"));
    }

    #[test]
    fn cancel_closes_the_question() {
        let td = tempfile::TempDir::new().unwrap();
        let (mut t, _) = loaded_with_orphan(&td);
        let q = orphan_question(&mut t);
        t.ask(q);
        t.answer(Answer::Cancel);
        assert!(t.pending.is_none());
    }

    fn row_named(t: &Tab, name: &str) -> Option<Row> {
        t.rows().iter().find(|r| r.search == name).cloned()
    }

    fn wait_for_row(t: &mut Tab, name: &str) -> Row {
        until(t, |t| row_named(t, name).is_some());
        row_named(t, name).unwrap_or_else(|| panic!("no row {name}"))
    }

    /// The transcript `with_session` wrote, wherever it was filed.
    fn transcript_exists(td: &tempfile::TempDir) -> bool {
        let root = dirs_under(td.path()).claude_projects;
        std::fs::read_dir(root).unwrap().flatten().any(|d| {
            d.path()
                .join(format!("{}.jsonl", crate::testing::SESSION_ID))
                .exists()
        })
    }

    /// A second worktree, on its own branch, beside the repo.
    fn side(repo: &Path) -> PathBuf {
        crate::testing::git(repo, &["worktree", "add", "-q", "-b", "side", "../side"]);
        zrush_core::paths::real(&repo.join("../side"))
    }

    #[test]
    fn a_new_worktree_needs_no_session() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let mut t = open(&td, &repo);
        let main = wait_for_row(&mut t, "main");
        t.new_worktree(Some(&main));
        assert!(
            matches!(
                &t.pending,
                Some(Pending::Branch {
                    at: None,
                    hand_over: None,
                    kind: None,
                    ..
                })
            ),
            "{:?}",
            t.pending
        );
        if let Some(Pending::Branch { kind, name, .. }) = &mut t.pending {
            *kind = Some(BranchKind::New);
            *name = "fresh".into();
        }
        t.create_branch();
        until_idle(&mut t);
        assert_eq!(t.error, None);
        t.refresh();
        wait_for_row(&mut t, "fresh");
    }

    #[test]
    fn a_new_worktree_from_a_session_takes_the_session() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        crate::testing::with_session(&td, &repo, &["hi", "hello"]);
        let mut t = open(&td, &repo);
        let row = wait_for_row(&mut t, crate::testing::SESSION_TITLE);
        t.new_worktree(Some(&row));
        let Some(Pending::Branch { hand_over, .. }) = &t.pending else {
            panic!("no branch question: {:?}", t.pending);
        };
        assert_eq!(
            hand_over.as_ref().map(|(a, i)| (a.as_str(), i.as_str())),
            Some(("claude", crate::testing::SESSION_ID))
        );
    }

    #[test]
    fn deleting_a_session_asks_first_then_deletes_its_transcript() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        crate::testing::with_session(&td, &repo, &["hi", "hello"]);
        let mut t = open(&td, &repo);
        let row = wait_for_row(&mut t, crate::testing::SESSION_TITLE);
        t.ask_delete(&row);
        assert!(matches!(t.pending, Some(Pending::Confirm(_))));
        assert!(transcript_exists(&td), "nothing goes before the answer");
        t.confirm(0);
        until_idle(&mut t);
        assert_eq!(t.error, None);
        assert!(!transcript_exists(&td));
    }

    #[test]
    fn cancelling_a_confirmation_does_nothing() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        crate::testing::with_session(&td, &repo, &["hi", "hello"]);
        let mut t = open(&td, &repo);
        let row = wait_for_row(&mut t, crate::testing::SESSION_TITLE);
        t.ask_delete(&row);
        let Some(Pending::Confirm(c)) = &t.pending else {
            panic!("no confirmation");
        };
        let cancel = c.choices().len() - 1;
        t.confirm(cancel);
        assert!(t.pending.is_none());
        assert!(t.busy.is_none());
        assert!(transcript_exists(&td));
    }

    #[test]
    fn removing_a_worktree_asks_then_removes_it() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let side = side(&repo);
        let mut t = open(&td, &repo);
        let row = wait_for_row(&mut t, "side");
        t.ask_delete(&row);
        assert!(matches!(t.pending, Some(Pending::Confirm(_))));
        t.confirm(0);
        until_idle(&mut t);
        assert_eq!(t.error, None);
        assert!(!side.exists());
    }

    #[test]
    fn the_main_worktree_is_never_offered_for_removal() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let mut t = open(&td, &repo);
        let main = wait_for_row(&mut t, "main");
        t.ask_delete(&main);
        assert!(t.pending.is_none());
        assert!(t.flash.as_deref().unwrap_or("").contains("main worktree"));
    }

    #[test]
    fn purging_removes_what_is_ticked() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let side = side(&repo);
        let mut t = open(&td, &repo);
        let row = wait_for_row(&mut t, "side");
        t.start_purge();
        t.toggle_mark(&row);
        assert!(t.is_marked(&row));
        t.ask_purge();
        let Some(Pending::Confirm(c)) = &t.pending else {
            panic!("no confirmation: {:?}", t.pending);
        };
        assert!(c.body().contains("1 worktree(s)"), "{}", c.body());
        t.confirm(1);
        until_idle(&mut t);
        assert!(!side.exists());
        assert!(t.purging.is_none(), "purge mode ends once it has run");
    }

    #[test]
    fn purging_nothing_says_so_instead_of_asking() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let mut t = open(&td, &repo);
        wait_for_row(&mut t, "main");
        t.start_purge();
        t.ask_purge();
        assert!(t.pending.is_none());
        assert!(t.flash.as_deref().unwrap_or("").contains("tick"));
    }
}
