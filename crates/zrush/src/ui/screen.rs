//! Putting the pieces on the terminal, and the loop that keeps them there.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::mpsc;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use zrush_core::app::Zrush;
use zrush_core::model::tree::{self, NodeId, RowKind, TreeInput};

use super::app::{Action, App, Mode};
use super::preview::Preview;
use super::{header, modal, preview, table, theme};

/// How wide the preview pane is. Below this the terminal is too narrow for
/// two panes and the list takes it all.
const PREVIEW_PERCENT: u16 = 50;
const MIN_WIDTH_FOR_PREVIEW: u16 = 100;

pub fn draw(f: &mut Frame, app: &App, z: &Zrush, prev: &Preview) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header::HEIGHT),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(f.area());

    let branch = app
        .worktrees
        .iter()
        .find(|w| w.is_main)
        .map_or("", |w| w.branch.as_str());
    let repo = z
        .main_root()
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    header::render(
        f,
        rows[0],
        &header::Info {
            repo: &repo,
            branch,
            agent: if app.active_agent.is_empty() {
                "none"
            } else {
                &app.active_agent
            },
            worktrees: app.worktrees.len(),
            live: app.live.len(),
            resumable: app.resumable.len(),
            loaded: app.scanned,
        },
    );

    let body = if rows[1].width >= MIN_WIDTH_FOR_PREVIEW {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(100 - PREVIEW_PERCENT),
                Constraint::Percentage(PREVIEW_PERCENT),
            ])
            .split(rows[1])
    } else {
        Layout::default()
            .constraints([Constraint::Percentage(100)])
            .split(rows[1])
    };

    let visible: Vec<zrush_core::model::tree::Row> = app
        .visible
        .iter()
        .filter_map(|&i| app.rows.get(i).cloned())
        .collect();
    let marked: HashSet<usize> = app
        .visible
        .iter()
        .enumerate()
        .filter(|(_, i)| app.marked.contains(i))
        .map(|(shown, _)| shown)
        .collect();
    let title = match app.mode {
        Mode::Purge => format!("Purge — {} marked", app.marked.len()),
        _ => format!("Worktrees({})", app.worktrees.len()),
    };
    table::render(
        f,
        body[0],
        &table::View {
            rows: &visible,
            cursor: app.cursor,
            marked: &marked,
            offset: app.offset,
        },
        &title,
    );
    if body.len() > 1 {
        preview::render(f, body[1], prev);
    }

    status_line(f, rows[2], app);

    if let Some(m) = &app.modal {
        modal::render(f, f.area(), m);
    }
}

fn status_line(f: &mut Frame, area: Rect, app: &App) {
    let left = match app.mode {
        Mode::Filter => format!("/{}▏", app.filter),
        Mode::Purge => "space marks · enter purges · esc cancels".into(),
        Mode::Normal if !app.filter.is_empty() => format!("/{}", app.filter),
        Mode::Normal => "<esc> quit".into(),
    };
    let right = app.flash.clone().unwrap_or_default();
    let gap =
        (area.width as usize).saturating_sub(left.chars().count() + right.chars().count() + 2);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {left}"), theme::header_label()),
            Span::raw(" ".repeat(gap)),
            Span::styled(right, theme::badge()),
        ])),
        area,
    );
}

/// Rebuild the rows from whatever has arrived so far.
pub fn rebuild(app: &mut App, z: &Zrush) {
    let mut sessions = app.live.clone();
    sessions.extend(app.resumable.clone());
    let new_root = z.new_root();
    let assigned = tree::assign(&sessions, &app.worktrees, z.main_root(), &new_root);

    // Until the scan has answered, the badge shows the cheap disk count.
    // Afterwards it counts what was actually assigned, so the two numbers
    // can never contradict each other.
    let history = if app.scanned {
        let mut counted: std::collections::HashMap<std::path::PathBuf, usize> =
            std::collections::HashMap::new();
        for a in &assigned {
            if let NodeId::Worktree(p) = &a.owner {
                *counted.entry(p.clone()).or_default() += 1;
            }
        }
        counted
    } else {
        app.history.clone()
    };

    let rows = tree::build(&TreeInput {
        worktrees: &app.worktrees,
        assigned: &assigned,
        statuses: &app.statuses,
        history: &history,
        collapsed: &app.collapsed,
        expanded: &app.expanded,
        resumable_max: app.resumable_max,
        show_all: app.show_all,
    });
    app.set_rows(rows);
}

/// What the preview pane should draw for the row under the cursor.
pub fn preview_for(app: &App, z: &Zrush) -> Preview {
    let Some(row) = app.current() else {
        return Preview::Empty;
    };
    match row.kind {
        RowKind::Session | RowKind::Orphan => {
            let Some(id) = row.session_id.as_deref() else {
                return Preview::Empty;
            };
            let agent = app
                .live
                .iter()
                .chain(app.resumable.iter())
                .find(|s| s.id == id)
                .map_or(app.active_agent.as_str(), |s| s.agent);
            Preview::Conversation(z.preview(agent, id))
        }
        RowKind::Worktree => {
            let NodeId::Worktree(p) = &row.node else {
                return Preview::Empty;
            };
            let mut lines: Vec<String> = Vec::new();
            if let Ok(out) = zrush_core::git::run(p, &["status", "--short", "--branch"]) {
                lines.extend(out.lines().take(15).map(str::to_string));
            }
            lines.push(String::new());
            if let Ok(out) =
                zrush_core::git::run(p, &["log", "--oneline", "--decorate", "-n", "10"])
            {
                lines.extend(out.lines().map(str::to_string));
            }
            Preview::Worktree(lines)
        }
        RowKind::Orphans | RowKind::More => Preview::Empty,
    }
}

/// Apply one event. Returns false when the interface should stop.
pub fn apply(
    app: &mut App,
    z: &Arc<Zrush>,
    probes: &crate::probe::Probes,
    event: crate::probe::Event,
) -> bool {
    use crate::probe::Event as E;

    // A result from a superseded reload must not paint over the new one.
    if event.generation().is_some_and(|g| g != probes.generation()) {
        return true;
    }

    match event {
        E::Key(k) => return handle(app, z, probes, k),
        E::Resize(..) => {}
        E::Worktrees(_, w) => {
            app.worktrees = w;
            app.scanned = false;
            rebuild(app, z);
        }
        E::Status(_, p, s) => {
            app.statuses.insert(p, s);
            rebuild(app, z);
        }
        E::History(_, p, n) => {
            app.history.insert(p, n);
            rebuild(app, z);
        }
        E::Live(_, s) => {
            app.live = s;
            rebuild(app, z);
        }
        E::Resumable(_, s) => {
            app.resumable = s;
            app.scanned = true;
            rebuild(app, z);
        }
        E::Failed(_, e) => app.flash(e),
    }
    true
}

fn handle(
    app: &mut App,
    z: &Arc<Zrush>,
    probes: &crate::probe::Probes,
    key: crossterm::event::KeyEvent,
) -> bool {
    app.flash = None;
    match app.on_key(key) {
        Action::Quit => return false,
        Action::None | Action::Redraw => {}
        Action::Reload => {
            probes.refresh(z, app.show_all);
        }
        Action::ReloadUncapped => {
            probes.refresh(z, true);
        }
        Action::Open { worktree, bind } => {
            let bind = bind.as_ref().map(|(a, i)| (a.as_str(), i.as_str()));
            report(app, z.open(&worktree, bind));
        }
        Action::RunAgent { worktree, resume } => run_agent(app, z, &worktree, resume.as_deref()),
        Action::CreateWorktree { name, hand_over } => {
            create(app, z, probes, &name, hand_over.as_ref());
        }
        Action::DeleteSession {
            agent,
            id,
            worktree,
        } => match z.delete_session(&agent, &id, false, worktree.as_deref()) {
            Ok(n) => {
                app.flash(format!("deleted {n} file(s)"));
                probes.refresh(z, app.show_all);
            }
            Err(e) => app.flash(e.to_string()),
        },
        Action::RemoveWorktree {
            path,
            with_sessions,
        } => {
            remove(app, z, probes, &path, with_sessions);
        }
        Action::Purge {
            worktrees,
            sessions,
        } => purge(app, z, probes, &worktrees, &sessions),
        Action::SwitchAgent(id) => switch_agent(app, z, probes, id),
    }
    true
}

fn report(app: &mut App, r: zrush_core::error::Result<()>) {
    if let Err(e) = r {
        app.flash(e.to_string());
    }
}

fn run_agent(app: &mut App, z: &Arc<Zrush>, worktree: &std::path::Path, resume: Option<&str>) {
    let Some(agent) = z.active_agent().map(zrush_core::agent::Agent::id) else {
        app.flash("no agent installed");
        return;
    };
    report(app, z.run_agent(worktree, agent, resume));
}

fn create(
    app: &mut App,
    z: &Arc<Zrush>,
    probes: &crate::probe::Probes,
    name: &str,
    hand_over: Option<&(String, String)>,
) {
    match z.create_worktree(name) {
        Ok(dest) => {
            let bind = hand_over.map(|(a, i)| (a.as_str(), i.as_str()));
            match z.open(&dest, bind) {
                Ok(()) => app.flash(format!("created {name}")),
                Err(e) => app.flash(e.to_string()),
            }
            probes.refresh(z, app.show_all);
        }
        Err(e) => app.flash(e.to_string()),
    }
}

fn remove(
    app: &mut App,
    z: &Arc<Zrush>,
    probes: &crate::probe::Probes,
    path: &std::path::Path,
    with_sessions: bool,
) {
    let mine: Vec<_> = app
        .live
        .iter()
        .chain(app.resumable.iter())
        .filter(|s| s.cwd.starts_with(path))
        .cloned()
        .collect();
    match z.remove_worktree(path, &mine, with_sessions) {
        Ok(r) => {
            app.flash(format!(
                "removed, {} transcript(s) deleted, {} running kept",
                r.transcripts, r.running_kept
            ));
            probes.refresh(z, app.show_all);
        }
        Err(e) => app.flash(e.to_string()),
    }
}

/// Each item is independent: a dirty worktree refuses without stopping the
/// rest, and only the closing summary reaches the status line.
fn purge(
    app: &mut App,
    z: &Arc<Zrush>,
    probes: &crate::probe::Probes,
    worktrees: &[std::path::PathBuf],
    sessions: &[(String, String)],
) {
    let mut removed = 0;
    let mut refused = 0;
    for p in worktrees {
        if z.remove_worktree(p, &[], false).is_ok() {
            removed += 1;
        } else {
            refused += 1;
        }
    }
    let gone: usize = sessions
        .iter()
        .map(|(a, i)| z.delete_session(a, i, false, None).unwrap_or(0))
        .sum();
    let kept = if refused > 0 {
        format!(", {refused} kept: git refused")
    } else {
        String::new()
    };
    app.flash(format!(
        "purged {removed} worktree(s), {gone} transcript(s){kept}"
    ));
    probes.refresh(z, app.show_all);
}

fn switch_agent(app: &mut App, z: &Arc<Zrush>, probes: &crate::probe::Probes, id: String) {
    match z.set_active_agent(&id) {
        Ok(()) => {
            app.active_agent = id;
            // Everything listed belonged to the agent we just left.
            app.live.clear();
            app.resumable.clear();
            app.scanned = false;
            rebuild(app, z);
            probes.refresh(z, app.show_all);
        }
        Err(e) => app.flash(e.to_string()),
    }
}

/// Run the interface until it is told to stop.
pub fn run(mut app: App, z: &Arc<Zrush>) -> zrush_core::error::Result<()> {
    let mut term = ratatui::init();
    let (tx, rx) = mpsc::channel();
    let probes = crate::probe::Probes::new(tx);
    probes.spawn_input();
    probes.refresh(z, false);

    let out = loop {
        let prev = preview_for(&app, z);
        if let Err(e) = term.draw(|f| draw(f, &app, z, &prev)) {
            break Err(e.into());
        }
        let height = term
            .size()
            .map_or(10, |s| s.height.saturating_sub(header::HEIGHT + 3));
        app.offset = table::scroll_to(app.offset, app.cursor, height as usize);

        match rx.recv() {
            Ok(event) => {
                if !apply(&mut app, z, &probes, event) {
                    break Ok(());
                }
            }
            Err(_) => break Ok(()),
        }
    };
    ratatui::restore();
    out
}
