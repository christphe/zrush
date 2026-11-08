//! What the app tests build on: keys, rows and a loaded `App`.

use super::*;
use zrush_core::model::{Session, SessionKind, Worktree};

pub(super) fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

pub(super) fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

pub(super) fn code(c: KeyCode) -> KeyEvent {
    KeyEvent::new(c, KeyModifiers::NONE)
}

pub(super) fn row(kind: RowKind, node: NodeId, id: Option<&str>, label: &str) -> Row {
    Row {
        kind,
        node,
        session_id: id.map(str::to_string),
        glyph: String::new(),
        label: label.into(),
        search: label.into(),
        badge: String::new(),
        status: String::new(),
        misfiled: false,
        location: String::new(),
    }
}

pub(super) fn wt(path: &str, is_main: bool) -> Worktree {
    Worktree {
        path: path.into(),
        branch: "b".into(),
        locked: false,
        prunable: false,
        is_main,
    }
}

pub(super) fn session(id: &str, kind: SessionKind) -> Session {
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
pub(super) fn app() -> App {
    let mut a = App::new(20, 5);
    a.snap.worktrees = vec![wt("/repo", true), wt("/repo/.claude/worktrees/x", false)];
    a.snap.resumable = vec![session("dead", SessionKind::Resumable)];
    a.snap.live = vec![session("alive", SessionKind::Live)];
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

/// The same key as `code`, but as the terminal reports it going up or
/// held down. Windows sends both for every key; Unix only presses.
pub(super) fn kind(c: KeyCode, kind: KeyEventKind) -> KeyEvent {
    KeyEvent::new_with_kind(c, KeyModifiers::NONE, kind)
}

/// enter is the key that could not work: the panel it opens cannot
/// resume a conversation filed somewhere else. So enter asks.
pub(super) fn with_misfiled() -> App {
    let mut a = app();
    let mut rows = a.rows.clone();
    rows[1].misfiled = true;
    a.set_rows(rows);
    a.cursor = 1;
    a
}

/// A session whose worktree is gone: the conversation outlived it, and
/// still carries the directory it was held in.
pub(super) fn with_orphan() -> App {
    let mut a = app();
    a.snap.resumable[0].cwd = "/repo/.claude/worktrees/gone".into();
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

/// ctrl-w asks which kind first; enter on the default takes "New
/// branch" and lands in the input.
pub(super) fn ask_new_branch(a: &mut App) {
    a.on_key(ctrl('w'));
    a.on_key(code(KeyCode::Enter));
}
