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
use super::modal::{CUSTOM, Choice, Modal};
use zrush_core::app::BranchKind;

/// The input modal that asks for an editor command line rather than a
/// branch name. Which one is up is told by its title, the way a confirm
/// says which question it answered.
pub const EDITOR_COMMAND: &str = "Editor command";

/// The two worktree inputs. Which one is up decides what enter means: a
/// name to cut a branch at, or a branch that already exists — and the
/// suggestion list only makes sense under the second.
/// Resuming an orphan, and the three places it can land.
pub const RESUME: &str = "Resume elsewhere";
pub const RESUME_HERE: &str = "In the main worktree";
pub const RESUME_ELSEWHERE: &str = "In another worktree…";
pub const RESUME_NEW: &str = "In a new worktree…";

pub const NEW_BRANCH: &str = "New branch";
pub const EXISTING_BRANCH: &str = "Existing branch";

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
    /// An editor by name, or a whole command line typed by hand.
    SetEditor(String),
    SetEditorCommand(Vec<String>),
    /// One setting, by the name `config.toml` gives it. Saved, reloaded,
    /// and applied — the interface never parses the value itself.
    SetSetting {
        key: String,
        value: String,
    },
    ToggleSetting(String),
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
    /// The config as the last reload left it, for the settings screen.
    pub cfg: zrush_core::config::Config,
    /// The settings screen is up, or something it opened is: a change made
    /// from it comes back to it rather than dropping you on the list.
    pub settings_open: bool,
    /// Which row it was on, so it comes back where it was.
    settings_at: usize,
    /// Branches a worktree could be opened on, for the suggestion list.
    /// Fetched with everything else, so the input is instant.
    pub branches: Vec<String>,
    pub agent_names: Vec<String>,
    pub active_agent: String,
    /// How the editor is named in the picker: `zed`, or the command line
    /// when one was typed.
    pub editor: String,
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
            busy: None,
            spin: 0,
            worktrees: Vec::new(),
            statuses: HashMap::new(),
            history: HashMap::new(),
            live: Vec::new(),
            resumable: Vec::new(),
            scanned: false,
            previews: HashMap::new(),
            preview_pending: HashSet::new(),
            cfg: zrush_core::config::Config::default(),
            settings_open: false,
            settings_at: 0,
            branches: Vec::new(),
            agent_names: Vec::new(),
            active_agent: String::new(),
            editor: String::new(),
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

    /// Every setting and what it is now. Rebuilt from the config each time
    /// rather than held: a change goes through the file, and this is what
    /// came back from it.
    pub fn open_settings(&mut self) {
        self.settings_open = true;
        self.modal = Some(Modal::Settings {
            rows: zrush_core::config::SETTINGS
                .iter()
                .map(|(key, _)| ((*key).to_string(), self.cfg.show(key)))
                .collect(),
            at: self.settings_at,
        });
    }

    /// Enter on a settings row, by what that row takes.
    fn edit_setting(&mut self, at: usize) -> Action {
        self.settings_at = at;
        let Some((key, kind)) = zrush_core::config::SETTINGS.get(at) else {
            return Action::Redraw;
        };
        match kind {
            zrush_core::config::Kind::Flag => Action::ToggleSetting((*key).to_string()),
            zrush_core::config::Kind::Editor => {
                self.open_editor_picker();
                Action::Redraw
            }
            zrush_core::config::Kind::Agent => {
                self.open_agent_picker(None);
                Action::Redraw
            }
            _ => {
                self.modal = Some(Modal::Input {
                    title: (*key).to_string(),
                    prompt: format!("{key}>"),
                    value: self.cfg.show(key),
                    suggestions: Vec::new(),
                    at: 0,
                });
                Action::Redraw
            }
        }
    }

    /// The branch input, in whichever of its two meanings. The suggestion
    /// list is the point of the second one, so it starts full rather than
    /// waiting for the first keystroke.
    fn open_branch_input(&mut self, existing: bool) {
        let suggestions = if existing {
            self.branches.clone()
        } else {
            Vec::new()
        };
        self.modal = Some(Modal::Input {
            title: if existing {
                EXISTING_BRANCH
            } else {
                NEW_BRANCH
            }
            .into(),
            prompt: "branch>".into(),
            value: String::new(),
            suggestions,
            at: 0,
        });
    }

    /// Narrow the branch list to what has been typed. Substring, not
    /// prefix: branches are named `fix/2358-thing` and nobody types the
    /// prefix they already know.
    fn refilter_suggestions(&mut self) {
        let all = self.branches.clone();
        let Some(Modal::Input {
            title,
            value,
            suggestions,
            at,
            ..
        }) = self.modal.as_mut()
        else {
            return;
        };
        if title != EXISTING_BRANCH {
            return;
        }
        let needle = value.to_lowercase();
        *suggestions = all
            .into_iter()
            .filter(|b| b.to_lowercase().contains(&needle))
            .collect();
        *at = 0;
    }

    /// The editors that get a `-n`, plus the way out: type the command.
    pub fn open_editor_picker(&mut self) {
        let mut editors: Vec<String> = zrush_core::config::KNOWN_EDITORS
            .iter()
            .map(|e| (*e).to_string())
            .collect();
        let at = editors.iter().position(|e| *e == self.editor).unwrap_or(0);
        editors.push(CUSTOM.to_string());
        self.modal = Some(Modal::EditorPicker { editors, at });
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
            // On a session row this resumes THAT conversation. It used to
            // pass None whatever the cursor was on, so a session row
            // started a fresh one — the core could resume all along and
            // nothing ever asked it to.
            (KeyCode::Char('o'), true) => {
                self.worktree_at_cursor()
                    .map_or(Action::None, |worktree| Action::RunAgent {
                        worktree,
                        session: self.session_at_cursor(),
                    })
            }
            (KeyCode::Char('w'), true) => {
                self.ask_branch_kind();
                Action::Redraw
            }
            (KeyCode::Char(','), false) => {
                self.open_settings();
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
            // An orphan's worktree is gone — removed, or never zrush's to
            // begin with. The conversation outlived it, so the question is
            // where to pick it up, and only you can answer that.
            RowKind::Orphan => self.ask_where(),
            RowKind::Orphans => Action::None,
        }
    }

    /// Asked before anything is typed: on the same word, "new" cuts a
    /// branch and "existing" opens one that is already there, possibly
    /// someone else's. A completion list under an input that also accepts
    /// new names cannot say which of the two it is offering.
    fn ask_branch_kind(&mut self) {
        self.modal = Some(Modal::Confirm {
            title: "New worktree".into(),
            body: "Which branch does it get?".into(),
            choices: vec![
                Choice::new(NEW_BRANCH),
                Choice::new(EXISTING_BRANCH),
                Choice::new("Cancel"),
            ],
            at: 0,
        });
    }

    /// Where to resume an orphan: the main worktree, another one, or a new
    /// one. Three answers because the third is a different question — it
    /// asks for a branch — and folding them together would hide that.
    fn ask_where(&mut self) -> Action {
        let Some(row) = self.current() else {
            return Action::None;
        };
        if self.session_at_cursor().is_none() {
            return Action::None;
        }
        let title = row.label.trim().to_string();
        let mut choices = vec![Choice::new(RESUME_HERE)];
        if self.worktrees.iter().filter(|w| !w.is_main).count() > 0 {
            choices.push(Choice::new(RESUME_ELSEWHERE));
        }
        choices.push(Choice::new(RESUME_NEW));
        choices.push(Choice::new("Cancel"));
        self.modal = Some(Modal::Confirm {
            title: RESUME.into(),
            body: format!("{title}\n\nIts worktree is gone. Where does it go?"),
            choices,
            at: 0,
        });
        Action::Redraw
    }

    /// The worktrees this session could be resumed in, main first, as the
    /// list shows them.
    fn open_worktree_picker(&mut self) {
        self.modal = Some(Modal::WorktreePicker {
            worktrees: self
                .worktrees
                .iter()
                .map(|w| {
                    let name = w
                        .path
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    if w.is_main {
                        format!("{name} [root]")
                    } else {
                        name
                    }
                })
                .collect(),
            at: 0,
        });
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
        // An error box has one answer: it goes away.
        if matches!(modal, Modal::Error { .. }) {
            self.modal = None;
            return Action::Redraw;
        }
        match key.code {
            KeyCode::Esc => {
                self.settings_open = false;
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
                self.refilter_suggestions();
                Action::Redraw
            }
            KeyCode::Char(c) => {
                match modal {
                    Modal::Input { value, .. } | Modal::Command { value } => {
                        value.push(c);
                        self.refilter_suggestions();
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
            Modal::Help | Modal::Error { .. } => Action::Redraw,
            Modal::Command { value } => self.run_command(&value),
            Modal::AgentPicker { agents, at, .. } => agents
                .get(at)
                .map_or(Action::Redraw, |a| Action::SwitchAgent(a.clone())),
            Modal::Input { title, value, .. }
                if zrush_core::config::SETTINGS
                    .iter()
                    .any(|(k, _)| *k == title) =>
            {
                Action::SetSetting { key: title, value }
            }
            Modal::Input { title, value, .. } if title == EDITOR_COMMAND => {
                let words: Vec<String> = value.split_whitespace().map(str::to_string).collect();
                if words.is_empty() {
                    return Action::Redraw;
                }
                Action::SetEditorCommand(words)
            }
            Modal::Input {
                title,
                value,
                suggestions,
                at,
                ..
            } => {
                let existing = title == EXISTING_BRANCH;
                // Under "existing", the highlighted branch wins: the list
                // is the answer. Under "new", what was typed is the whole
                // point and there is no list.
                let name = if existing {
                    suggestions.get(at).cloned().unwrap_or(value)
                } else {
                    value
                };
                if name.trim().is_empty() {
                    return Action::Redraw;
                }
                // On a session row, ctrl-w hands that session to the new
                // worktree; that is the one thing the removed "+ new
                // worktree" row could never do.
                Action::CreateWorktree {
                    name,
                    kind: if existing {
                        BranchKind::Existing
                    } else {
                        BranchKind::New
                    },
                    hand_over: self.session_at_cursor(),
                }
            }
            Modal::Settings { at, .. } => self.edit_setting(at),
            Modal::WorktreePicker { at, .. } => {
                let bind = self.session_at_cursor();
                self.worktrees
                    .get(at)
                    .map_or(Action::Redraw, |w| Action::Open {
                        worktree: w.path.clone(),
                        bind,
                    })
            }
            Modal::EditorPicker { editors, at } => match editors.get(at).map(String::as_str) {
                None => Action::Redraw,
                // Not an editor: the row that asks for a command line.
                Some(CUSTOM) => {
                    self.modal = Some(Modal::Input {
                        title: EDITOR_COMMAND.into(),
                        prompt: "cmd>".into(),
                        value: self.editor.clone(),
                        suggestions: Vec::new(),
                        at: 0,
                    });
                    Action::Redraw
                }
                Some(e) => Action::SetEditor(e.to_string()),
            },
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
            RESUME => {
                let bind = self.session_at_cursor();
                match choices.get(at).map(|c| c.label.as_str()) {
                    Some(RESUME_HERE) => {
                        self.worktrees
                            .iter()
                            .find(|w| w.is_main)
                            .map_or(Action::Redraw, |w| Action::Open {
                                worktree: w.path.clone(),
                                bind,
                            })
                    }
                    Some(RESUME_ELSEWHERE) => {
                        self.open_worktree_picker();
                        Action::Redraw
                    }
                    // The same question ctrl-w asks, and the same answer:
                    // the session is handed to whatever it creates.
                    Some(RESUME_NEW) => {
                        self.ask_branch_kind();
                        Action::Redraw
                    }
                    _ => Action::Redraw,
                }
            }
            "New worktree" => {
                let existing = choices.get(at).is_some_and(|c| c.label == EXISTING_BRANCH);
                self.open_branch_input(existing);
                Action::Redraw
            }
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
            // `:editor code` names one; `:editor code --profile x` is a
            // whole command line, which is a different setting.
            (Some("editor" | "e"), Some(_)) => {
                let rest: Vec<String> = value
                    .split_whitespace()
                    .skip(1)
                    .map(str::to_string)
                    .collect();
                match rest.len() {
                    1 => Action::SetEditor(rest[0].clone()),
                    _ => Action::SetEditorCommand(rest),
                }
            }
            (Some("settings"), _) => {
                self.open_settings();
                Action::Redraw
            }
            (Some("set"), Some(key)) => {
                let value = value
                    .split_whitespace()
                    .skip(2)
                    .collect::<Vec<_>>()
                    .join(" ");
                Action::SetSetting {
                    key: key.to_string(),
                    value,
                }
            }
            (Some("editor" | "e"), None) => {
                self.open_editor_picker();
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
    fn an_error_box_goes_away_on_any_key() {
        let mut a = app();
        a.error("Remove worktree", "fatal: it contains untracked files");
        assert!(matches!(a.modal, Some(Modal::Error { .. })));
        assert_eq!(a.on_key(key('j')), Action::Redraw);
        assert!(a.modal.is_none());
        // And it did not move the cursor on the way out.
        assert_eq!(a.cursor, 0);
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

    // --------------------------------------------------------- orphans ---

    /// A session whose worktree is gone: the conversation outlived it.
    fn with_orphan() -> App {
        let mut a = app();
        let mut rows = a.rows.clone();
        rows.push(row(
            RowKind::Orphan,
            NodeId::Orphans,
            Some("dead"),
            "old work",
        ));
        a.set_rows(rows);
        a.cursor = a.visible.len() - 1;
        a
    }

    #[test]
    fn enter_on_an_orphan_asks_where_instead_of_doing_nothing() {
        let mut a = with_orphan();
        assert_eq!(a.on_key(code(KeyCode::Enter)), Action::Redraw);
        match a.modal.as_ref().unwrap() {
            Modal::Confirm {
                title, choices, at, ..
            } => {
                assert_eq!(title, RESUME);
                assert_eq!(choices[*at].label, RESUME_HERE);
                let labels: Vec<&str> = choices.iter().map(|c| c.label.as_str()).collect();
                assert_eq!(
                    labels,
                    vec![RESUME_HERE, RESUME_ELSEWHERE, RESUME_NEW, "Cancel"]
                );
            }
            other => panic!("wrong modal: {other:?}"),
        }
    }

    #[test]
    fn resuming_here_binds_it_to_the_main_worktree() {
        let mut a = with_orphan();
        a.on_key(code(KeyCode::Enter));
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::Open {
                worktree: "/repo".into(),
                bind: Some(("claude".into(), "dead".into())),
            }
        );
    }

    #[test]
    fn resuming_elsewhere_offers_the_worktrees_and_binds_to_the_chosen_one() {
        let mut a = with_orphan();
        a.on_key(code(KeyCode::Enter));
        a.on_key(code(KeyCode::Down));
        assert_eq!(a.on_key(code(KeyCode::Enter)), Action::Redraw);
        match a.modal.as_ref().unwrap() {
            Modal::WorktreePicker { worktrees, .. } => {
                assert_eq!(worktrees, &vec!["repo [root]".to_string(), "x".to_string()]);
            }
            other => panic!("wrong modal: {other:?}"),
        }
        a.on_key(code(KeyCode::Down));
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::Open {
                worktree: "/repo/.claude/worktrees/x".into(),
                bind: Some(("claude".into(), "dead".into())),
            }
        );
    }

    /// The third answer is a different question — which branch — so it
    /// hands over to the one ctrl-w already asks.
    #[test]
    fn resuming_in_a_new_worktree_asks_for_the_branch() {
        let mut a = with_orphan();
        a.on_key(code(KeyCode::Enter));
        a.on_key(code(KeyCode::Down));
        a.on_key(code(KeyCode::Down));
        a.on_key(code(KeyCode::Enter));
        match a.modal.as_ref().unwrap() {
            Modal::Confirm { title, .. } => assert_eq!(title, "New worktree"),
            other => panic!("wrong modal: {other:?}"),
        }
        a.on_key(code(KeyCode::Enter));
        for c in "rescue".chars() {
            a.on_key(key(c));
        }
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::CreateWorktree {
                name: "rescue".into(),
                kind: BranchKind::New,
                hand_over: Some(("claude".into(), "dead".into())),
            }
        );
    }

    // -------------------------------------------------------- settings ---

    #[test]
    fn a_comma_opens_the_settings_screen_with_the_live_values() {
        let mut a = app();
        a.cfg.editor = "code".into();
        a.cfg.resumable_max = 9;
        a.on_key(key(','));
        match a.modal.as_ref().unwrap() {
            Modal::Settings { rows, at } => {
                assert_eq!(*at, 0);
                assert_eq!(rows.len(), zrush_core::config::SETTINGS.len());
                assert!(rows.contains(&("editor".into(), "code".into())));
                assert!(rows.contains(&("resumable_max".into(), "9".into())));
            }
            other => panic!("wrong modal: {other:?}"),
        }
    }

    #[test]
    fn a_flag_is_toggled_in_place_rather_than_typed() {
        let mut a = app();
        a.on_key(key(','));
        // titles is the fifth row.
        for _ in 0..4 {
            a.on_key(code(KeyCode::Down));
        }
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::ToggleSetting("titles".into())
        );
    }

    #[test]
    fn a_number_is_typed_and_comes_back_under_its_own_name() {
        let mut a = app();
        a.cfg.resumable_max = 5;
        a.on_key(key(','));
        for _ in 0..6 {
            a.on_key(code(KeyCode::Down));
        }
        a.on_key(code(KeyCode::Enter));
        match a.modal.as_ref().unwrap() {
            // The current value is there to edit, not an empty field.
            Modal::Input { title, value, .. } => {
                assert_eq!(title, "resumable_max");
                assert_eq!(value, "5");
            }
            other => panic!("wrong modal: {other:?}"),
        }
        a.on_key(code(KeyCode::Backspace));
        a.on_key(key('8'));
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::SetSetting {
                key: "resumable_max".into(),
                value: "8".into(),
            }
        );
    }

    #[test]
    fn the_editor_row_opens_the_picker_that_also_takes_a_command() {
        let mut a = app();
        a.on_key(key(','));
        a.on_key(code(KeyCode::Enter));
        match a.modal.as_ref().unwrap() {
            Modal::EditorPicker { editors, .. } => {
                assert_eq!(editors.last().map(String::as_str), Some(CUSTOM));
            }
            other => panic!("wrong modal: {other:?}"),
        }
        assert!(a.settings_open, "the change should come back to settings");
    }

    #[test]
    fn set_from_the_command_bar_takes_the_rest_of_the_line() {
        let mut a = app();
        a.on_key(key(':'));
        for c in "set terminal open -a Ghostty".chars() {
            a.on_key(key(c));
        }
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::SetSetting {
                key: "terminal".into(),
                value: "open -a Ghostty".into(),
            }
        );
    }

    #[test]
    fn escape_leaves_the_settings_screen_for_good() {
        let mut a = app();
        a.on_key(key(','));
        a.on_key(code(KeyCode::Esc));
        assert!(!a.settings_open);
        assert!(a.modal.is_none());
    }

    // ---------------------------------------------------------- editor ---

    #[test]
    fn editor_with_one_word_names_an_editor() {
        let mut a = app();
        a.on_key(key(':'));
        for c in "editor code".chars() {
            a.on_key(key(c));
        }
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::SetEditor("code".into())
        );
    }

    #[test]
    fn editor_with_arguments_is_a_whole_command_line() {
        let mut a = app();
        a.on_key(key(':'));
        for c in "editor code --reuse-window".chars() {
            a.on_key(key(c));
        }
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::SetEditorCommand(vec!["code".into(), "--reuse-window".into()])
        );
    }

    #[test]
    fn editor_on_its_own_offers_the_list_with_the_current_one_under_the_cursor() {
        let mut a = app();
        a.editor = "code".into();
        a.on_key(key(':'));
        for c in "editor".chars() {
            a.on_key(key(c));
        }
        a.on_key(code(KeyCode::Enter));
        match a.modal.as_ref().unwrap() {
            Modal::EditorPicker { editors, at } => {
                assert_eq!(editors.last().map(String::as_str), Some(CUSTOM));
                assert_eq!(editors[*at], "code");
            }
            other => panic!("wrong modal: {other:?}"),
        }
    }

    #[test]
    fn the_custom_row_asks_for_a_command_instead_of_settling() {
        let mut a = app();
        a.open_editor_picker();
        // Up from the first row wraps onto the last, which is the custom one.
        a.on_key(code(KeyCode::Up));
        assert_eq!(a.on_key(code(KeyCode::Enter)), Action::Redraw);
        match a.modal.as_ref().unwrap() {
            Modal::Input { title, .. } => assert_eq!(title, EDITOR_COMMAND),
            other => panic!("wrong modal: {other:?}"),
        }
        for c in "hx -n".chars() {
            a.on_key(key(c));
        }
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::SetEditorCommand(vec!["hx".into(), "-n".into()])
        );
    }

    #[test]
    fn an_empty_editor_command_changes_nothing() {
        let mut a = app();
        a.open_editor_picker();
        a.on_key(code(KeyCode::Up));
        a.on_key(code(KeyCode::Enter));
        assert_eq!(a.on_key(code(KeyCode::Enter)), Action::Redraw);
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
    fn ctrl_o_on_a_worktree_starts_a_fresh_session() {
        let mut a = app();
        assert_eq!(
            a.on_key(ctrl('o')),
            Action::RunAgent {
                worktree: "/repo".into(),
                session: None,
            }
        );
    }

    /// The core could resume from the start; the interface never asked, so
    /// ctrl-o on a session row opened a new conversation instead of the
    /// one under the cursor.
    #[test]
    fn ctrl_o_on_a_session_resumes_that_one_in_its_worktree() {
        let mut a = app();
        a.on_key(code(KeyCode::Down));
        assert_eq!(
            a.on_key(ctrl('o')),
            Action::RunAgent {
                worktree: "/repo".into(),
                session: Some(("claude".into(), "dead".into())),
            }
        );
    }

    /// ctrl-w asks which kind first; enter on the default takes "New
    /// branch" and lands in the input.
    fn ask_new_branch(a: &mut App) {
        a.on_key(ctrl('w'));
        a.on_key(code(KeyCode::Enter));
    }

    #[test]
    fn ctrl_w_on_a_session_hands_it_to_the_new_worktree() {
        let mut a = app();
        a.on_key(code(KeyCode::Down));
        ask_new_branch(&mut a);
        for c in "feat".chars() {
            a.on_key(key(c));
        }
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::CreateWorktree {
                name: "feat".into(),
                kind: BranchKind::New,
                hand_over: Some(("claude".into(), "dead".into())),
            }
        );
    }

    #[test]
    fn an_empty_branch_name_creates_nothing() {
        let mut a = app();
        ask_new_branch(&mut a);
        assert_eq!(a.on_key(code(KeyCode::Enter)), Action::Redraw);
    }

    // ------------------------------------------------- new worktree ---

    #[test]
    fn ctrl_w_asks_which_kind_of_branch_before_anything_is_typed() {
        let mut a = app();
        a.on_key(ctrl('w'));
        match a.modal.as_ref().unwrap() {
            Modal::Confirm {
                title, choices, at, ..
            } => {
                assert_eq!(title, "New worktree");
                assert_eq!(choices[*at].label, NEW_BRANCH);
                assert_eq!(choices[1].label, EXISTING_BRANCH);
            }
            other => panic!("wrong modal: {other:?}"),
        }
    }

    #[test]
    fn a_new_branch_gets_no_suggestions_to_be_mistaken_for_a_choice() {
        let mut a = app();
        a.branches = vec!["main".into(), "feat".into()];
        ask_new_branch(&mut a);
        match a.modal.as_ref().unwrap() {
            Modal::Input {
                title, suggestions, ..
            } => {
                assert_eq!(title, NEW_BRANCH);
                assert!(suggestions.is_empty());
            }
            other => panic!("wrong modal: {other:?}"),
        }
    }

    #[test]
    fn an_existing_branch_offers_the_list_and_narrows_it_as_you_type() {
        let mut a = app();
        a.branches = vec!["main".into(), "fix/2358-reader".into(), "feat".into()];
        a.on_key(ctrl('w'));
        a.on_key(code(KeyCode::Down));
        a.on_key(code(KeyCode::Enter));
        match a.modal.as_ref().unwrap() {
            Modal::Input {
                title, suggestions, ..
            } => {
                assert_eq!(title, EXISTING_BRANCH);
                assert_eq!(suggestions.len(), 3);
            }
            other => panic!("wrong modal: {other:?}"),
        }
        // Substring, not prefix: nobody types the part of the name they
        // already know.
        for c in "2358".chars() {
            a.on_key(key(c));
        }
        match a.modal.as_ref().unwrap() {
            Modal::Input { suggestions, .. } => {
                assert_eq!(suggestions, &vec!["fix/2358-reader".to_string()]);
            }
            other => panic!("wrong modal: {other:?}"),
        }
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::CreateWorktree {
                name: "fix/2358-reader".into(),
                kind: BranchKind::Existing,
                hand_over: None,
            }
        );
    }

    #[test]
    fn cancelling_the_kind_question_opens_no_input() {
        let mut a = app();
        a.on_key(ctrl('w'));
        a.on_key(code(KeyCode::Up)); // onto Cancel
        assert_eq!(a.on_key(code(KeyCode::Enter)), Action::Redraw);
        assert!(a.modal.is_none());
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
