//! The state the interface holds, and what a key does to it.
//!
//! `on_key` is a pure function of `(state, key) -> Action`: it mutates
//! `App` and says what the caller should go and do. Nothing here touches
//! git, an agent or the terminal, so the whole keyboard is testable without
//! any of them.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use zrush_core::model::tree::{NodeId, Row, RowKind};
use zrush_core::model::{GitStatus, Session, Worktree};

use super::filter;
use super::modal::{Choice, Modal};

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
    /// Start an agent in a terminal of its own.
    RunAgent {
        worktree: PathBuf,
        resume: Option<String>,
    },
    CreateWorktree {
        name: String,
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
    Purge {
        worktrees: Vec<PathBuf>,
        sessions: Vec<(String, String)>,
    },
    SwitchAgent(String),
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

    pub worktrees: Vec<Worktree>,
    pub statuses: HashMap<PathBuf, GitStatus>,
    pub history: HashMap<PathBuf, usize>,
    pub live: Vec<Session>,
    pub resumable: Vec<Session>,
    pub scanned: bool,
    /// Previews already fetched, and the ones a worker is fetching. Both
    /// are keyed by `preview_key`, so moving the cursor back onto a row
    /// costs nothing.
    pub previews: HashMap<String, crate::ui::preview::Preview>,
    pub preview_pending: HashSet<String>,

    /// Every agent installed here, and which one is being listed. The
    /// header shows it, and `:agent` picks from it.
    pub agent_names: Vec<String>,
    pub active_agent: String,
    /// Shown as `~` in the path column. Held here rather than read from the
    /// environment inside the renderer: drawing should not depend on what
    /// the process happens to be able to see.
    pub home: Option<PathBuf>,

    pub more_step: usize,
    pub resumable_max: usize,
    /// The row the cursor sat on before the last rebuild, so a reload does
    /// not throw you back to the top.
    anchor: Option<(NodeId, Option<String>)>,
}

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
            worktrees: Vec::new(),
            statuses: HashMap::new(),
            history: HashMap::new(),
            live: Vec::new(),
            resumable: Vec::new(),
            scanned: false,
            previews: HashMap::new(),
            preview_pending: HashSet::new(),
            agent_names: Vec::new(),
            active_agent: String::new(),
            home: std::env::var_os("HOME").map(PathBuf::from),
            more_step,
            resumable_max,
            anchor: None,
        }
    }

    /// Identifies a row for the preview cache: a session by its id, a
    /// worktree by its path. Stable across a reload.
    pub fn preview_key(row: &Row) -> Option<String> {
        match row.kind {
            RowKind::Session | RowKind::Orphan => {
                row.session_id.as_ref().map(|id| format!("s:{id}"))
            }
            RowKind::Worktree => match &row.node {
                NodeId::Worktree(p) => Some(format!("w:{}", p.display())),
                NodeId::Orphans => None,
            },
            RowKind::Orphans | RowKind::More => None,
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
            .live
            .iter()
            .chain(self.resumable.iter())
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
        self.live.iter().any(|s| s.id == id)
    }

    /// The agent question: at startup when nothing settled it, and any
    /// time through `:agent`.
    pub fn open_agent_picker(&mut self, reason: Option<String>) {
        let at = self
            .agent_names
            .iter()
            .position(|a| *a == self.active_agent)
            .unwrap_or(0);
        self.modal = Some(Modal::AgentPicker {
            agents: self.agent_names.clone(),
            at,
            reason,
        });
    }

    pub fn flash(&mut self, msg: impl Into<String>) {
        self.flash = Some(msg.into());
    }

    // ------------------------------------------------------------- keys ---

    pub fn on_key(&mut self, key: KeyEvent) -> Action {
        // A modal owns the keyboard while it is up. No key means two things.
        if self.modal.is_some() {
            return self.modal_key(key);
        }
        match self.mode {
            Mode::Filter => self.filter_key(key),
            Mode::Purge => self.purge_key(key),
            Mode::Normal => self.normal_key(key),
        }
    }

    fn normal_key(&mut self, key: KeyEvent) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match (key.code, ctrl) {
            (KeyCode::Char('c' | 'q'), true) | (KeyCode::Esc, _) => Action::Quit,
            (KeyCode::Up | KeyCode::Char('k'), false) => {
                self.move_by(-1);
                Action::Redraw
            }
            (KeyCode::Down | KeyCode::Char('j'), false) => {
                self.move_by(1);
                Action::Redraw
            }
            (KeyCode::Left | KeyCode::Char('h'), false) => self.fold(true),
            (KeyCode::Right | KeyCode::Char('l'), false) => self.fold(false),
            (KeyCode::Char('l'), true) => Action::Reload,
            (KeyCode::Enter, _) => self.enter(),
            (KeyCode::Char('o'), true) => {
                self.worktree_at_cursor()
                    .map_or(Action::None, |worktree| Action::RunAgent {
                        worktree,
                        resume: None,
                    })
            }
            (KeyCode::Char('w'), true) => {
                self.modal = Some(Modal::Input {
                    title: "New worktree".into(),
                    prompt: "branch>".into(),
                    value: String::new(),
                    suggestions: Vec::new(),
                    at: 0,
                });
                Action::Redraw
            }
            (KeyCode::Char('d'), true) => self.ask_delete(),
            (KeyCode::Char('p'), true) => {
                // Marking rows you cannot see would be a trap: unfold first.
                self.collapsed.clear();
                self.show_all = true;
                self.mode = Mode::Purge;
                self.marked.clear();
                Action::ReloadUncapped
            }
            (KeyCode::Char('/'), false) => {
                self.mode = Mode::Filter;
                Action::Redraw
            }
            (KeyCode::Char(':'), false) => {
                self.modal = Some(Modal::Command {
                    value: String::new(),
                });
                Action::Redraw
            }
            (KeyCode::Char('?'), false) => {
                self.modal = Some(Modal::Help);
                Action::Redraw
            }
            _ => Action::None,
        }
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

    fn enter(&mut self) -> Action {
        let Some(row) = self.current() else {
            return Action::None;
        };
        match row.kind {
            // Not an action: it raises this node's cap and redraws, so enter
            // after enter keeps unfolding.
            RowKind::More => {
                let node = row.node.clone();
                let cur = self
                    .expanded
                    .get(&node)
                    .copied()
                    .unwrap_or(self.resumable_max);
                self.expanded.insert(node, cur + self.more_step);
                Action::ReloadUncapped
            }
            // A worktree always means a fresh session; a session means
            // resume that one.
            RowKind::Worktree => {
                self.worktree_at_cursor()
                    .map_or(Action::None, |worktree| Action::Open {
                        worktree,
                        bind: None,
                    })
            }
            RowKind::Session => {
                let Some(worktree) = self.worktree_at_cursor() else {
                    return Action::None;
                };
                self.session_at_cursor()
                    .map_or(Action::None, |bind| Action::Open {
                        worktree,
                        bind: Some(bind),
                    })
            }
            // An orphan has no worktree to open. ctrl-w hands it to a new one.
            RowKind::Orphan | RowKind::Orphans => Action::None,
        }
    }

    fn ask_delete(&mut self) -> Action {
        let Some(row) = self.current() else {
            return Action::None;
        };
        match row.kind {
            RowKind::Session | RowKind::Orphan => {
                let Some((_, id)) = self.session_at_cursor() else {
                    return Action::None;
                };
                if self.is_running(&id) {
                    self.flash("that session is running; stop it first");
                    return Action::Redraw;
                }
                let title = row.label.trim().to_string();
                self.modal = Some(Modal::Confirm {
                    title: "Delete session".into(),
                    body: format!("{title}\n\nIts transcript goes for good."),
                    choices: vec![Choice::destructive("Delete it"), Choice::new("Cancel")],
                    at: 1,
                });
                Action::Redraw
            }
            RowKind::Worktree => {
                let Some(path) = self.worktree_at_cursor() else {
                    return Action::None;
                };
                let node = NodeId::Worktree(path.clone());
                let n = self
                    .rows
                    .iter()
                    .filter(|r| r.node == node && r.is_child())
                    .count();
                let name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let choices = if n == 0 {
                    vec![Choice::destructive("Remove it"), Choice::new("Cancel")]
                } else {
                    vec![
                        Choice::new("Remove the worktree, keep the sessions"),
                        Choice::destructive("Remove it and delete the sessions too"),
                        Choice::new("Cancel"),
                    ]
                };
                self.modal = Some(Modal::Confirm {
                    title: "Remove worktree".into(),
                    body: if n == 0 {
                        format!("{name} has no sessions.")
                    } else {
                        format!("{name} has {n} session(s).")
                    },
                    at: choices.len() - 1,
                    choices,
                });
                Action::Redraw
            }
            RowKind::More | RowKind::Orphans => Action::None,
        }
    }

    fn filter_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => {
                self.filter.clear();
                self.mode = Mode::Normal;
                self.refilter();
                Action::Redraw
            }
            KeyCode::Enter => {
                self.mode = Mode::Normal;
                Action::Redraw
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.refilter();
                Action::Redraw
            }
            KeyCode::Up => {
                self.move_by(-1);
                Action::Redraw
            }
            KeyCode::Down => {
                self.move_by(1);
                Action::Redraw
            }
            KeyCode::Char(c) => {
                self.filter.push(c);
                self.refilter();
                Action::Redraw
            }
            _ => Action::None,
        }
    }

    fn purge_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => {
                self.leave_purge();
                Action::Reload
            }
            KeyCode::Char(' ') => {
                if let Some(&i) = self.visible.get(self.cursor)
                    && !self.marked.insert(i)
                {
                    self.marked.remove(&i);
                }
                self.move_by(1);
                Action::Redraw
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_by(-1);
                Action::Redraw
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_by(1);
                Action::Redraw
            }
            KeyCode::Enter => self.ask_purge(),
            _ => Action::None,
        }
    }

    fn leave_purge(&mut self) {
        self.mode = Mode::Normal;
        self.marked.clear();
        self.show_all = false;
    }

    fn ask_purge(&mut self) -> Action {
        let (worktrees, sessions, kept_live, skipped_main) = self.purge_plan();
        if worktrees.is_empty() && sessions.is_empty() {
            self.flash("nothing to purge: mark rows with space first");
            return Action::Redraw;
        }
        let mut note = String::new();
        if kept_live > 0 {
            use std::fmt::Write as _;
            let _ = write!(note, "\n{kept_live} running session(s) kept.");
        }
        if skipped_main > 0 {
            note.push_str("\nThe main worktree is skipped.");
        }
        self.modal = Some(Modal::Confirm {
            title: "Purge".into(),
            body: format!(
                "{} worktree(s) and {} session(s).{note}",
                worktrees.len(),
                sessions.len()
            ),
            choices: vec![
                Choice::destructive("Purge, and their other sessions too"),
                Choice::destructive("Purge only what is marked"),
                Choice::new("Cancel"),
            ],
            at: 2,
        });
        Action::Redraw
    }

    /// Marked rows into a plan. A session whose worktree is also marked
    /// goes with the worktree, so the count never says it twice.
    fn purge_plan(&self) -> (Vec<PathBuf>, Vec<(String, String)>, usize, usize) {
        let main = self
            .worktrees
            .iter()
            .find(|w| w.is_main)
            .map(|w| w.path.clone());
        let mut worktrees = Vec::new();
        let mut sessions: Vec<(String, String, Option<PathBuf>)> = Vec::new();
        let mut kept_live = 0;
        let mut skipped_main = 0;

        for &i in &self.marked {
            let Some(row) = self.rows.get(i) else {
                continue;
            };
            match row.kind {
                RowKind::Worktree => {
                    let NodeId::Worktree(p) = &row.node else {
                        continue;
                    };
                    if Some(p) == main.as_ref() {
                        skipped_main += 1;
                    } else {
                        worktrees.push(p.clone());
                    }
                }
                RowKind::Session | RowKind::Orphan => {
                    let Some(id) = row.session_id.clone() else {
                        continue;
                    };
                    if self.is_running(&id) {
                        kept_live += 1;
                        continue;
                    }
                    let agent = self
                        .live
                        .iter()
                        .chain(self.resumable.iter())
                        .find(|s| s.id == id)
                        .map_or_else(String::new, |s| s.agent.to_string());
                    let owner = match &row.node {
                        NodeId::Worktree(p) => Some(p.clone()),
                        NodeId::Orphans => None,
                    };
                    sessions.push((agent, id, owner));
                }
                RowKind::Orphans | RowKind::More => {}
            }
        }
        let sessions = sessions
            .into_iter()
            .filter(|(_, _, owner)| !owner.as_ref().is_some_and(|p| worktrees.contains(p)))
            .map(|(a, i, _)| (a, i))
            .collect();
        (worktrees, sessions, kept_live, skipped_main)
    }

    fn modal_key(&mut self, key: KeyEvent) -> Action {
        let Some(modal) = self.modal.as_mut() else {
            return Action::None;
        };
        match key.code {
            KeyCode::Esc => {
                self.modal = None;
                Action::Redraw
            }
            KeyCode::Up => {
                modal.move_by(-1);
                Action::Redraw
            }
            KeyCode::Down => {
                modal.move_by(1);
                Action::Redraw
            }
            KeyCode::Backspace => {
                match modal {
                    Modal::Input { value, .. } | Modal::Command { value } => {
                        value.pop();
                    }
                    _ => {}
                }
                Action::Redraw
            }
            KeyCode::Char(c) => {
                match modal {
                    Modal::Input { value, .. } | Modal::Command { value } => {
                        value.push(c);
                        return Action::Redraw;
                    }
                    Modal::Help => {
                        self.modal = None;
                        return Action::Redraw;
                    }
                    _ => {}
                }
                Action::None
            }
            KeyCode::Enter => self.modal_enter(),
            _ => Action::None,
        }
    }

    fn modal_enter(&mut self) -> Action {
        let Some(modal) = self.modal.take() else {
            return Action::None;
        };
        match modal {
            Modal::Help => Action::Redraw,
            Modal::Command { value } => self.run_command(&value),
            Modal::AgentPicker { agents, at, .. } => agents
                .get(at)
                .map_or(Action::Redraw, |a| Action::SwitchAgent(a.clone())),
            Modal::Input {
                value,
                suggestions,
                at,
                ..
            } => {
                // The highlighted branch if there is one, otherwise what was
                // typed.
                let name = suggestions.get(at).cloned().unwrap_or(value);
                if name.trim().is_empty() {
                    return Action::Redraw;
                }
                // On a session row, ctrl-w hands that session to the new
                // worktree; that is the one thing the removed "+ new
                // worktree" row could never do.
                Action::CreateWorktree {
                    name,
                    hand_over: self.session_at_cursor(),
                }
            }
            Modal::Confirm {
                title, choices, at, ..
            } => self.confirmed(&title, &choices, at),
        }
    }

    fn confirmed(&mut self, title: &str, choices: &[Choice], at: usize) -> Action {
        let last = choices.len() - 1;
        if at == last {
            if title == "Purge" {
                self.leave_purge();
                return Action::Reload;
            }
            return Action::Redraw;
        }
        match title {
            "Delete session" => self
                .session_at_cursor()
                .map_or(Action::Redraw, |(agent, id)| Action::DeleteSession {
                    agent,
                    id,
                    worktree: self.worktree_at_cursor(),
                }),
            "Remove worktree" => {
                self.worktree_at_cursor()
                    .map_or(Action::Redraw, |path| Action::RemoveWorktree {
                        path,
                        with_sessions: choices.len() == 3 && at == 1,
                    })
            }
            "Purge" => {
                let (worktrees, sessions, ..) = self.purge_plan();
                self.leave_purge();
                Action::Purge {
                    worktrees,
                    sessions,
                }
            }
            _ => Action::Redraw,
        }
    }

    fn run_command(&mut self, value: &str) -> Action {
        let mut words = value.split_whitespace();
        match (words.next(), words.next()) {
            (Some("q" | "quit"), _) => Action::Quit,
            (Some("help" | "h"), _) => {
                self.modal = Some(Modal::Help);
                Action::Redraw
            }
            (Some("agent" | "a"), Some(name)) => Action::SwitchAgent(name.to_string()),
            // `:agent` on its own offers the list rather than doing nothing.
            (Some("agent" | "a"), None) => {
                self.open_agent_picker(None);
                Action::Redraw
            }
            (Some(other), _) => {
                self.flash(format!("unknown command: {other}"));
                Action::Redraw
            }
            (None, _) => Action::Redraw,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zrush_core::model::SessionKind;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn code(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    fn row(kind: RowKind, node: NodeId, id: Option<&str>, label: &str) -> Row {
        Row {
            kind,
            node,
            session_id: id.map(str::to_string),
            glyph: String::new(),
            label: label.into(),
            search: label.into(),
            badge: String::new(),
            status: String::new(),
            location: String::new(),
        }
    }

    fn wt(path: &str, is_main: bool) -> Worktree {
        Worktree {
            path: path.into(),
            branch: "b".into(),
            locked: false,
            prunable: false,
            is_main,
        }
    }

    fn session(id: &str, kind: SessionKind) -> Session {
        Session {
            agent: "claude",
            id: id.into(),
            title: "t".into(),
            status: String::new(),
            cwd: "/repo".into(),
            launch_cwd: "/repo".into(),
            kind,
            last_activity: 0,
        }
    }

    /// One worktree with one dead session, plus an orphan node.
    fn app() -> App {
        let mut a = App::new(20, 5);
        a.worktrees = vec![wt("/repo", true), wt("/repo/.claude/worktrees/x", false)];
        a.resumable = vec![session("dead", SessionKind::Resumable)];
        a.live = vec![session("alive", SessionKind::Live)];
        a.set_rows(vec![
            row(
                RowKind::Worktree,
                NodeId::Worktree("/repo".into()),
                None,
                "main [root]",
            ),
            row(
                RowKind::Session,
                NodeId::Worktree("/repo".into()),
                Some("dead"),
                "old work",
            ),
            row(
                RowKind::Worktree,
                NodeId::Worktree("/repo/.claude/worktrees/x".into()),
                None,
                "x",
            ),
            row(
                RowKind::Session,
                NodeId::Worktree("/repo/.claude/worktrees/x".into()),
                Some("alive"),
                "running",
            ),
        ]);
        a
    }

    // ----------------------------------------------------- moving about ---

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

    // ------------------------------------------------------------ enter ---

    #[test]
    fn enter_on_a_worktree_opens_it_with_no_binding() {
        let mut a = app();
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::Open {
                worktree: "/repo".into(),
                bind: None
            }
        );
    }

    #[test]
    fn enter_on_a_session_binds_it_first() {
        let mut a = app();
        a.on_key(code(KeyCode::Down));
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::Open {
                worktree: "/repo".into(),
                bind: Some(("claude".into(), "dead".into()))
            }
        );
    }

    #[test]
    fn enter_on_a_more_row_raises_the_cap_instead_of_acting() {
        let mut a = app();
        a.set_rows(vec![
            row(
                RowKind::Worktree,
                NodeId::Worktree("/repo".into()),
                None,
                "main",
            ),
            row(
                RowKind::More,
                NodeId::Worktree("/repo".into()),
                None,
                "[…more]",
            ),
        ]);
        a.on_key(code(KeyCode::Down));
        assert_eq!(a.on_key(code(KeyCode::Enter)), Action::ReloadUncapped);
        assert_eq!(a.expanded.get(&NodeId::Worktree("/repo".into())), Some(&25));
    }

    // ----------------------------------------------------------- modals ---

    #[test]
    fn a_modal_owns_the_keyboard_while_it_is_up() {
        let mut a = app();
        a.on_key(key('?'));
        // j would move the cursor in normal mode; here it closes the help.
        let before = a.cursor;
        a.on_key(key('j'));
        assert_eq!(a.cursor, before);
    }

    #[test]
    fn escape_closes_a_modal_rather_than_quitting() {
        let mut a = app();
        a.on_key(key('?'));
        assert_eq!(a.on_key(code(KeyCode::Esc)), Action::Redraw);
        assert!(a.modal.is_none());
        // Now it quits.
        assert_eq!(a.on_key(code(KeyCode::Esc)), Action::Quit);
    }

    #[test]
    fn the_delete_confirmation_starts_on_the_safe_answer() {
        let mut a = app();
        a.on_key(code(KeyCode::Down));
        a.on_key(ctrl('d'));
        match a.modal.as_ref().unwrap() {
            Modal::Confirm { choices, at, .. } => {
                assert_eq!(*at, choices.len() - 1, "the cursor must start on Cancel");
                assert_eq!(choices[*at].label, "Cancel");
            }
            other => panic!("wrong modal: {other:?}"),
        }
    }

    #[test]
    fn cancelling_a_deletion_does_nothing() {
        let mut a = app();
        a.on_key(code(KeyCode::Down));
        a.on_key(ctrl('d'));
        assert_eq!(a.on_key(code(KeyCode::Enter)), Action::Redraw);
    }

    #[test]
    fn confirming_a_deletion_names_the_session_and_its_worktree() {
        let mut a = app();
        a.on_key(code(KeyCode::Down));
        a.on_key(ctrl('d'));
        a.on_key(code(KeyCode::Up)); // onto "Delete it"
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::DeleteSession {
                agent: "claude".into(),
                id: "dead".into(),
                worktree: Some("/repo".into()),
            }
        );
    }

    #[test]
    fn a_running_session_refuses_deletion_before_it_asks() {
        let mut a = app();
        a.cursor = 3; // the live one
        a.on_key(ctrl('d'));
        assert!(a.modal.is_none());
        assert!(a.flash.unwrap().contains("running"));
    }

    #[test]
    fn removing_a_worktree_offers_keeping_its_sessions() {
        let mut a = app();
        a.on_key(ctrl('d'));
        match a.modal.as_ref().unwrap() {
            Modal::Confirm { choices, .. } => assert_eq!(choices.len(), 3),
            other => panic!("wrong modal: {other:?}"),
        }
    }

    #[test]
    fn a_worktree_with_no_sessions_gets_a_two_answer_dialog() {
        let mut a = app();
        a.set_rows(vec![row(
            RowKind::Worktree,
            NodeId::Worktree("/repo".into()),
            None,
            "main",
        )]);
        a.on_key(ctrl('d'));
        match a.modal.as_ref().unwrap() {
            Modal::Confirm { choices, .. } => assert_eq!(choices.len(), 2),
            other => panic!("wrong modal: {other:?}"),
        }
    }

    // ----------------------------------------------------------- filter ---

    #[test]
    fn typing_in_filter_mode_narrows_the_list() {
        let mut a = app();
        a.on_key(key('/'));
        assert_eq!(a.mode, Mode::Filter);
        a.on_key(key('x'));
        assert!(a.visible.len() < a.rows.len());
    }

    #[test]
    fn escaping_the_filter_restores_every_row() {
        let mut a = app();
        a.on_key(key('/'));
        a.on_key(key('x'));
        a.on_key(code(KeyCode::Esc));
        assert_eq!(a.mode, Mode::Normal);
        assert_eq!(a.visible.len(), a.rows.len());
    }

    #[test]
    fn backspace_widens_the_filter_again() {
        let mut a = app();
        a.on_key(key('/'));
        a.on_key(key('x'));
        let narrow = a.visible.len();
        a.on_key(code(KeyCode::Backspace));
        assert!(a.visible.len() > narrow);
    }

    // ------------------------------------------------------------ purge ---

    #[test]
    fn purge_mode_unfolds_everything_first() {
        let mut a = app();
        a.collapsed.insert(NodeId::Worktree("/repo".into()));
        assert_eq!(a.on_key(ctrl('p')), Action::ReloadUncapped);
        assert_eq!(a.mode, Mode::Purge);
        assert!(
            a.collapsed.is_empty(),
            "marking rows you cannot see would be a trap"
        );
        assert!(a.show_all);
    }

    #[test]
    fn space_marks_a_row_and_moves_on() {
        let mut a = app();
        a.on_key(ctrl('p'));
        a.on_key(code(KeyCode::Char(' ')));
        assert!(a.marked.contains(&0));
        assert_eq!(a.cursor, 1);
    }

    #[test]
    fn space_again_unmarks_it() {
        let mut a = app();
        a.on_key(ctrl('p'));
        a.on_key(code(KeyCode::Char(' ')));
        a.on_key(code(KeyCode::Up));
        a.on_key(code(KeyCode::Char(' ')));
        assert!(a.marked.is_empty());
    }

    #[test]
    fn purging_nothing_says_so_instead_of_asking() {
        let mut a = app();
        a.on_key(ctrl('p'));
        a.on_key(code(KeyCode::Enter));
        assert!(a.modal.is_none());
        assert!(a.flash.unwrap().contains("mark rows"));
    }

    #[test]
    fn the_main_worktree_is_skipped_by_a_purge() {
        let mut a = app();
        a.on_key(ctrl('p'));
        a.on_key(code(KeyCode::Char(' '))); // the main worktree
        let (worktrees, _, _, skipped) = a.purge_plan();
        assert!(worktrees.is_empty());
        assert_eq!(skipped, 1);
    }

    #[test]
    fn a_running_session_is_never_purged() {
        let mut a = app();
        a.on_key(ctrl('p'));
        a.cursor = 3;
        a.on_key(code(KeyCode::Char(' ')));
        let (_, sessions, kept_live, _) = a.purge_plan();
        assert!(sessions.is_empty());
        assert_eq!(kept_live, 1);
    }

    #[test]
    fn a_session_going_with_its_worktree_is_not_counted_twice() {
        let mut a = app();
        a.on_key(ctrl('p'));
        a.cursor = 2;
        a.on_key(code(KeyCode::Char(' '))); // worktree x
        a.on_key(code(KeyCode::Char(' '))); // its live session, which is kept
        // Now a dead session under a marked worktree.
        a.resumable.push(session("dead2", SessionKind::Resumable));
        let mut rows = a.rows.clone();
        rows.push(row(
            RowKind::Session,
            NodeId::Worktree("/repo/.claude/worktrees/x".into()),
            Some("dead2"),
            "old",
        ));
        a.set_rows(rows);
        a.marked.insert(4);
        let (worktrees, sessions, ..) = a.purge_plan();
        assert_eq!(worktrees.len(), 1);
        assert!(sessions.is_empty(), "it goes with the worktree");
    }

    #[test]
    fn escaping_purge_mode_clears_the_marks() {
        let mut a = app();
        a.on_key(ctrl('p'));
        a.on_key(code(KeyCode::Char(' ')));
        a.on_key(code(KeyCode::Esc));
        assert_eq!(a.mode, Mode::Normal);
        assert!(a.marked.is_empty());
        assert!(!a.show_all);
    }

    // --------------------------------------------------------- commands ---

    #[test]
    fn the_command_bar_quits() {
        let mut a = app();
        a.on_key(key(':'));
        a.on_key(key('q'));
        assert_eq!(a.on_key(code(KeyCode::Enter)), Action::Quit);
    }

    #[test]
    fn the_command_bar_switches_agent() {
        let mut a = app();
        a.on_key(key(':'));
        for c in "agent codex".chars() {
            a.on_key(key(c));
        }
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::SwitchAgent("codex".into())
        );
    }

    #[test]
    fn an_unknown_command_says_so_rather_than_doing_something() {
        let mut a = app();
        a.on_key(key(':'));
        a.on_key(key('z'));
        assert_eq!(a.on_key(code(KeyCode::Enter)), Action::Redraw);
        assert!(a.flash.unwrap().contains("unknown command"));
    }

    // ---------------------------------------------------------- folding ---

    #[test]
    fn left_folds_and_right_unfolds() {
        let mut a = app();
        a.on_key(code(KeyCode::Left));
        assert!(a.collapsed.contains(&NodeId::Worktree("/repo".into())));
        a.on_key(code(KeyCode::Right));
        assert!(a.collapsed.is_empty());
    }

    // ----------------------------------------------------------- others ---

    #[test]
    fn ctrl_o_starts_an_agent_without_touching_the_binding() {
        let mut a = app();
        assert_eq!(
            a.on_key(ctrl('o')),
            Action::RunAgent {
                worktree: "/repo".into(),
                resume: None
            }
        );
    }

    #[test]
    fn ctrl_w_on_a_session_hands_it_to_the_new_worktree() {
        let mut a = app();
        a.on_key(code(KeyCode::Down));
        a.on_key(ctrl('w'));
        for c in "feat".chars() {
            a.on_key(key(c));
        }
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::CreateWorktree {
                name: "feat".into(),
                hand_over: Some(("claude".into(), "dead".into())),
            }
        );
    }

    #[test]
    fn an_empty_branch_name_creates_nothing() {
        let mut a = app();
        a.on_key(ctrl('w'));
        assert_eq!(a.on_key(code(KeyCode::Enter)), Action::Redraw);
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

#[cfg(test)]
mod agent_tests {
    use super::tests_helpers::*;
    use super::*;

    #[test]
    fn the_command_bar_with_no_name_offers_the_list() {
        let mut a = App::new(20, 5);
        a.agent_names = vec!["claude".into(), "codex".into()];
        a.active_agent = "codex".into();
        a.on_key(chr(':'));
        a.on_key(chr('a'));
        a.on_key(key(KeyCode::Enter));
        match a.modal.as_ref().unwrap() {
            Modal::AgentPicker { agents, at, .. } => {
                assert_eq!(agents.len(), 2);
                assert_eq!(*at, 1, "the cursor starts on the active agent");
            }
            other => panic!("wrong modal: {other:?}"),
        }
    }

    #[test]
    fn picking_from_the_list_switches() {
        let mut a = App::new(20, 5);
        a.agent_names = vec!["claude".into(), "codex".into()];
        a.active_agent = "claude".into();
        a.open_agent_picker(None);
        a.on_key(key(KeyCode::Down));
        assert_eq!(
            a.on_key(key(KeyCode::Enter)),
            Action::SwitchAgent("codex".into())
        );
    }

    #[test]
    fn the_picker_can_say_why_it_is_asking() {
        let mut a = App::new(20, 5);
        a.agent_names = vec!["claude".into()];
        a.open_agent_picker(Some("codex is configured but not installed here".into()));
        match a.modal.as_ref().unwrap() {
            Modal::AgentPicker { reason, .. } => {
                assert!(reason.as_ref().unwrap().contains("not installed"));
            }
            other => panic!("wrong modal: {other:?}"),
        }
    }
}

#[cfg(test)]
mod tests_helpers {
    use super::*;

    pub fn chr(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    pub fn key(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }
}
