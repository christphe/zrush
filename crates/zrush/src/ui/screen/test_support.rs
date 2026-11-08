//! What the screen tests build on: a loaded `App` and a drawn frame.

use super::*;
pub(super) use crate::testing::{git, scratch_repo, zrush};
use crate::ui::app::App;
use zrush_core::model::{Session, SessionKind};

pub(super) fn key(code: crossterm::event::KeyCode) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::from(code)
}

pub(super) fn session(id: &str, title: &str, cwd: &str, kind: SessionKind) -> Session {
    Session {
        agent: "claude",
        id: id.into(),
        title: title.into(),
        status: match kind {
            SessionKind::Live => "busy 0m".into(),
            SessionKind::Resumable => "resumable 2d".into(),
        },
        cwd: cwd.into(),
        launch_cwd: cwd.into(),
        kind,
        last_activity: 0,
    }
}

/// A full frame as text, at a fixed size so the layout is the thing
/// under test rather than the terminal.
pub(super) fn frame(app: &App, z: &Zrush, w: u16, h: u16) -> String {
    let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
    let prev = preview_for(app);
    term.draw(|f| draw(f, app, z, &prev)).unwrap();
    crate::ui::tests_support::flatten(term.backend())
}

pub(super) fn loaded(td: &tempfile::TempDir) -> (Arc<Zrush>, App) {
    let repo = scratch_repo(td);
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            ".claude/worktrees/rust",
            "-b",
            "rust",
        ],
    );
    let z = zrush(td, repo.clone());

    let mut app = App::new(20, 5);
    app.active_agent = "claude".into();
    app.agent_names = vec!["claude".into()];
    app.snap.worktrees = z.worktrees().unwrap();
    app.snap.live = vec![session(
        "aaaaaaaa-1111-2222-3333-444455556666",
        "Conversion en Rust",
        repo.join(".claude/worktrees/rust").to_str().unwrap(),
        SessionKind::Live,
    )];
    app.snap.resumable = vec![session(
        "bbbbbbbb-1111-2222-3333-444455556666",
        "Publier zrush sur GitHub",
        repo.to_str().unwrap(),
        SessionKind::Resumable,
    )];
    app.snap.scanned = true;
    rebuild(&mut app, &z);
    (z, app)
}
