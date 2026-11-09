//! The state the interface holds, and what a key does to it.
//!
//! `on_key` is a pure function of `(state, key) -> Action`: it mutates
//! `App` and says what the caller should go and do. Nothing here touches
//! git, an agent or the terminal, so the whole keyboard is testable without
//! any of them.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use zrush_core::model::tree::{NodeId, Row, RowKind};

use super::modal::{CUSTOM, Choice, Modal};
use zrush_core::app::BranchKind;
use zrush_core::filter;
use zrush_core::intent::{self, Intent, Outcome};
use zrush_core::snapshot::{RepoSnapshot, TreeView};

/// The input modal that asks for an editor command line rather than a
/// branch name. Which one is up is told by its title, the way a confirm
/// says which question it answered.
pub const EDITOR_COMMAND: &str = "Editor command";

/// The two worktree inputs. Which one is up decides what enter means: a
/// name to cut a branch at, or a branch that already exists — and the
/// suggestion list only makes sense under the second.
pub use zrush_core::intent::{
    MISFILED, MISFILED_ANYWAY, MISFILED_FILE, RESUME, RESUME_RECREATE, RESUME_ROOT,
};
pub const UPDATE: &str = "Update zrush";
pub const UPDATE_NOW: &str = "Update now";
pub const UPDATE_SKIP: &str = "Skip this version";

pub use zrush_core::intent::{EXISTING_BRANCH, NEW_BRANCH};

/// What a question is about, read once when it is asked rather than again
/// when it is answered.
///
/// Every probe rebuilds the rows, including while a modal is up, and the
/// cursor follows the row it was on only while that row still exists. Read
/// at confirm time, "the session under the cursor" could be a different
/// one — and the answer to "delete it?" would land on that one.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Pending {
    session: Option<(String, String)>,
    worktree: Option<PathBuf>,
    /// Where a recreated worktree must land: an orphan's own old path.
    at: Option<PathBuf>,
    /// The misfiled or orphan question that is up, as core asked it.
    question: Option<intent::Question>,
}

/// What the event loop must do once a key has been handled. Anything with a
/// side effect lives here rather than in `App`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
    Redraw,
    /// Re-run every probe.
    Reload,
    /// Re-run them with the transcript scan unbounded.
    ReloadUncapped,
    /// Clear the association and open, or bind this session and open.
    Open {
        worktree: PathBuf,
        bind: Option<(String, String)>,
    },
    /// Start an agent in a terminal of its own. `session` names the
    /// conversation to resume, with the agent it belongs to — which is not
    /// always the one being listed.
    RunAgent {
        worktree: PathBuf,
        session: Option<(String, String)>,
    },
    CreateWorktree {
        name: String,
        kind: zrush_core::app::BranchKind,
        /// Where it goes, when it is not the path the branch name implies:
        /// recreating an orphan's worktree means its exact old one.
        at: Option<PathBuf>,
        hand_over: Option<(String, String)>,
    },
    DeleteSession {
        agent: String,
        id: String,
        worktree: Option<PathBuf>,
    },
    RemoveWorktree {
        path: PathBuf,
        with_sessions: bool,
    },
    Purge(zrush_core::purge::Plan),
    SwitchAgent(String),
    /// An editor by name, or a whole command line typed by hand.
    SetEditor(String),
    SetEditorCommand(Vec<String>),
    /// Refile an orphan under a worktree and bind it there.
    AdoptSession {
        agent: String,
        id: String,
        worktree: PathBuf,
    },
    /// One setting, by the name `config.toml` gives it. Saved, reloaded,
    /// and applied — the interface never parses the value itself.
    SetSetting {
        key: String,
        value: String,
    },
    ToggleSetting(String),
    /// Leave the interface and run the installer for this tag.
    Update(String),
    /// Never offer this tag again.
    SkipUpdate(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    /// `/` — typing narrows the list.
    Filter,
    /// `ctrl-p` — space marks rows, enter removes the lot.
    Purge,
}

pub struct App {
    pub rows: Vec<Row>,
    /// Indices into `rows` that survive the filter, in order.
    pub visible: Vec<usize>,
    /// Alongside `visible`: which characters of each row's name the filter
    /// matched. Empty means the row came along with a neighbour.
    pub matched: Vec<Vec<usize>>,
    pub cursor: usize,
    pub offset: usize,
    pub mode: Mode,
    pub modal: Option<Modal>,
    pub filter: String,
    pub marked: HashSet<usize>,
    pub collapsed: HashSet<NodeId>,
    pub expanded: HashMap<NodeId, usize>,
    pub show_all: bool,
    pub flash: Option<String>,
    /// What a worker is doing right now, for the spinner. `None` means the
    /// interface is idle and a long action may start.
    pub busy: Option<String>,
    /// Which frame of the spinner to draw. Bumped by the loop's timeout,
    /// never by a key.
    pub spin: usize,

    /// The repository as loaded so far, previews included.
    pub snap: RepoSnapshot,

    /// Every agent installed here, and which one is being listed. The
    /// header shows it, and `:agent` picks from it.
    /// The config as the last reload left it, for the settings screen.
    pub cfg: zrush_core::config::Config,
    /// The settings screen is up, or something it opened is: a change made
    /// from it comes back to it rather than dropping you on the list.
    pub settings_open: bool,
    /// Which row it was on, so it comes back where it was.
    settings_at: usize,
    /// What the open question is about, captured when it was asked.
    pending: Pending,
    pub agent_names: Vec<String>,
    pub active_agent: String,
    /// How the editor is named in the picker: `zed`, or the command line
    /// when one was typed.
    pub editor: String,
    /// Shown as `~` in the path column. Held here rather than read from the
    /// environment inside the renderer: drawing should not depend on what
    /// the process happens to be able to see.
    pub home: Option<PathBuf>,

    /// A newer release the check found, until it is installed, skipped or
    /// put off.
    pub update: Option<String>,
    /// The tag to install once the interface is down, when the answer was
    /// to update now.
    pub install: Option<String>,

    pub more_step: usize,
    pub resumable_max: usize,
    /// The row the cursor sat on before the last rebuild, so a reload does
    /// not throw you back to the top.
    anchor: Option<(NodeId, Option<String>)>,
}

mod ask;
mod command;
mod dialogs;
mod keys;
mod pickers;
mod purge;
#[cfg(test)]
mod test_support;

impl App {
    pub fn new(more_step: usize, resumable_max: usize) -> Self {
        Self {
            rows: Vec::new(),
            visible: Vec::new(),
            matched: Vec::new(),
            cursor: 0,
            offset: 0,
            mode: Mode::Normal,
            modal: None,
            filter: String::new(),
            marked: HashSet::new(),
            collapsed: HashSet::new(),
            expanded: HashMap::new(),
            show_all: false,
            flash: None,
            busy: None,
            spin: 0,
            snap: RepoSnapshot::default(),
            cfg: zrush_core::config::Config::default(),
            settings_open: false,
            settings_at: 0,
            pending: Pending::default(),
            agent_names: Vec::new(),
            active_agent: String::new(),
            editor: String::new(),
            home: std::env::var_os("HOME").map(PathBuf::from),
            more_step,
            resumable_max,
            update: None,
            install: None,
            anchor: None,
        }
    }

    /// Identifies a row for the preview cache: see
    /// `zrush_core::snapshot::preview_key`.
    pub fn preview_key(row: &Row) -> Option<String> {
        zrush_core::snapshot::preview_key(row)
    }

    /// How the tree is laid out right now, for the snapshot to build.
    pub fn tree_view(&self) -> TreeView {
        TreeView {
            collapsed: self.collapsed.clone(),
            expanded: self.expanded.clone(),
            resumable_max: self.resumable_max,
            show_all: self.show_all,
        }
    }

    pub fn current(&self) -> Option<&Row> {
        self.visible
            .get(self.cursor)
            .and_then(|&i| self.rows.get(i))
    }

    /// Replace the rows and put the cursor back on the row it was on.
    pub fn set_rows(&mut self, rows: Vec<Row>) {
        self.rows = rows;
        self.refilter();
        if let Some((node, id)) = &self.anchor
            && let Some(at) = self
                .visible
                .iter()
                .position(|&i| &self.rows[i].node == node && &self.rows[i].session_id == id)
        {
            self.cursor = at;
        }
        self.clamp();
    }

    pub fn refilter(&mut self) {
        let hits = filter::hits(&self.rows, &self.filter);
        self.visible = hits.iter().map(|h| h.index).collect();
        self.matched = hits.into_iter().map(|h| h.matched).collect();
        self.clamp();
    }

    fn clamp(&mut self) {
        if self.visible.is_empty() {
            self.cursor = 0;
        } else if self.cursor >= self.visible.len() {
            self.cursor = self.visible.len() - 1;
        }
    }

    fn remember(&mut self) {
        self.anchor = self
            .current()
            .map(|r| (r.node.clone(), r.session_id.clone()));
    }

    fn move_by(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        let len = self.visible.len() as isize;
        self.cursor = (self.cursor as isize + delta).rem_euclid(len) as usize;
        self.remember();
    }

    /// The session the cursor is on, if it is on one.
    fn session_at_cursor(&self) -> Option<(String, String)> {
        let row = self.current()?;
        let id = row.session_id.clone()?;
        let agent = self
            .snap
            .live
            .iter()
            .chain(self.snap.resumable.iter())
            .find(|s| s.id == id)
            .map(|s| s.agent.to_string())?;
        Some((agent, id))
    }

    fn worktree_at_cursor(&self) -> Option<PathBuf> {
        match &self.current()?.node {
            NodeId::Worktree(p) => Some(p.clone()),
            NodeId::Orphans => None,
        }
    }

    fn is_running(&self, id: &str) -> bool {
        self.snap.live.iter().any(|s| s.id == id)
    }

    pub fn flash(&mut self, msg: impl Into<String>) {
        self.flash = Some(msg.into());
    }

    /// An action that failed says so in a box: git's reason is at the end
    /// of a line the status line cannot hold, and a truncated error is
    /// worse than none — it reads as if nothing happened.
    pub fn error(&mut self, title: impl Into<String>, body: impl std::fmt::Display) {
        self.modal = Some(Modal::Error {
            title: title.into(),
            body: body.to_string(),
        });
    }

    fn fold(&mut self, shut: bool) -> Action {
        let Some(row) = self.current() else {
            return Action::None;
        };
        let node = row.node.clone();
        if shut {
            self.collapsed.insert(node);
        } else {
            self.collapsed.remove(&node);
        }
        self.remember();
        Action::Reload
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    #[test]
    fn the_cursor_wraps_at_both_ends() {
        let mut a = app();
        assert_eq!(a.cursor, 0);
        a.on_key(code(KeyCode::Up));
        assert_eq!(a.cursor, 3);
        a.on_key(code(KeyCode::Down));
        assert_eq!(a.cursor, 0);
    }

    #[test]
    fn a_key_coming_back_up_does_nothing() {
        let mut a = app();
        a.on_key(code(KeyCode::Down));
        assert_eq!(
            a.on_key(kind(KeyCode::Down, KeyEventKind::Release)),
            Action::None
        );
        assert_eq!(a.cursor, 1);
        // Not even Esc, which would otherwise quit.
        assert_eq!(
            a.on_key(kind(KeyCode::Esc, KeyEventKind::Release)),
            Action::None
        );
    }

    #[test]
    fn a_key_held_down_keeps_moving() {
        let mut a = app();
        a.on_key(code(KeyCode::Down));
        a.on_key(kind(KeyCode::Down, KeyEventKind::Repeat));
        assert_eq!(a.cursor, 2);
    }

    #[test]
    fn vim_keys_move_too() {
        let mut a = app();
        a.on_key(key('j'));
        assert_eq!(a.cursor, 1);
        a.on_key(key('k'));
        assert_eq!(a.cursor, 0);
    }

    #[test]
    fn a_reload_puts_the_cursor_back_on_the_row_it_was_on() {
        let mut a = app();
        a.on_key(code(KeyCode::Down));
        a.on_key(code(KeyCode::Down));
        let before = a.current().unwrap().node.clone();
        let rows = a.rows.clone();
        // A row appears above, as a new worktree would.
        let mut grown = vec![row(
            RowKind::Worktree,
            NodeId::Worktree("/new".into()),
            None,
            "new",
        )];
        grown.extend(rows);
        a.set_rows(grown);
        assert_eq!(a.current().unwrap().node, before);
    }

    #[test]
    fn left_folds_and_right_unfolds() {
        let mut a = app();
        a.on_key(code(KeyCode::Left));
        assert!(a.collapsed.contains(&NodeId::Worktree("/repo".into())));
        a.on_key(code(KeyCode::Right));
        assert!(a.collapsed.is_empty());
    }

    #[test]
    fn an_empty_list_never_indexes_out_of_bounds() {
        let mut a = App::new(20, 5);
        a.set_rows(Vec::new());
        assert_eq!(a.on_key(code(KeyCode::Down)), Action::Redraw);
        assert_eq!(a.on_key(code(KeyCode::Enter)), Action::None);
        assert_eq!(a.on_key(ctrl('d')), Action::None);
        assert!(a.current().is_none());
    }
}
