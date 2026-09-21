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

/// The user's home directory, shown as `~` in the path column.
fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}

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
    let home = dirs_home();
    let title = match app.mode {
        Mode::Purge => format!("Purge — {} marked", app.marked.len()),
        _ => format!("Worktrees({})", app.worktrees.len()),
    };
    table::render(
        f,
        body[0],
        &table::View {
            rows: &visible,
            matched: &app.matched,
            home: home.as_deref(),
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
        Mode::Filter => format!(
            "/{}▏   fuzzy · \x27sub · =exact · ^start · end$ · !not",
            app.filter
        ),
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

/// What the preview pane should draw for the row under the cursor: whatever
/// has been fetched for it, and nothing while a worker is still on it.
pub fn preview_for(app: &App) -> Preview {
    app.current()
        .and_then(App::preview_key)
        .and_then(|k| app.previews.get(&k).cloned())
        .unwrap_or(Preview::Empty)
}

/// Ask for the preview of the row under the cursor, once.
pub fn want_preview(app: &mut App, z: &Arc<Zrush>, probes: &crate::probe::Probes) {
    let Some(row) = app.current().cloned() else {
        return;
    };
    let Some(key) = App::preview_key(&row) else {
        return;
    };
    if app.previews.contains_key(&key) || !app.preview_pending.insert(key.clone()) {
        return;
    }
    let job = match row.kind {
        RowKind::Session | RowKind::Orphan => {
            let Some(id) = row.session_id.clone() else {
                return;
            };
            let agent = app
                .live
                .iter()
                .chain(app.resumable.iter())
                .find(|s| s.id == id)
                .map_or(app.active_agent.as_str(), |s| s.agent)
                .to_string();
            crate::probe::PreviewJob::Conversation { agent, id }
        }
        RowKind::Worktree => match row.node {
            NodeId::Worktree(p) => crate::probe::PreviewJob::Worktree(p),
            NodeId::Orphans => return,
        },
        RowKind::Orphans | RowKind::More => return,
    };
    probes.request_preview(z, key, job);
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
            // Whatever was fetched described the previous state of the repo.
            app.previews.clear();
            app.preview_pending.clear();
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
        E::Preview(_, key, preview) => {
            app.preview_pending.remove(&key);
            app.previews.insert(key, preview);
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
        Action::SwitchTheme(name) => {
            crate::ui::theme::set(crate::ui::theme::parse(&name));
            app.flash(format!("theme: {name}"));
        }
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
        want_preview(&mut app, z, &probes);
        let prev = preview_for(&app);
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

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::testing::{git, scratch_repo, zrush};
    use crate::ui::app::App;
    use zrush_core::model::{Session, SessionKind};

    fn session(id: &str, title: &str, cwd: &str, kind: SessionKind) -> Session {
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
    pub fn frame(app: &App, z: &Zrush, w: u16, h: u16) -> String {
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        let prev = preview_for(app);
        term.draw(|f| draw(f, app, z, &prev)).unwrap();
        crate::ui::tests_support::flatten(term.backend())
    }

    pub fn loaded(td: &tempfile::TempDir) -> (Arc<Zrush>, App) {
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
        app.worktrees = z.worktrees().unwrap();
        app.live = vec![session(
            "aaaaaaaa-1111-2222-3333-444455556666",
            "Conversion en Rust",
            repo.join(".claude/worktrees/rust").to_str().unwrap(),
            SessionKind::Live,
        )];
        app.resumable = vec![session(
            "bbbbbbbb-1111-2222-3333-444455556666",
            "Publier zrush sur GitHub",
            repo.to_str().unwrap(),
            SessionKind::Resumable,
        )];
        app.scanned = true;
        rebuild(&mut app, &z);
        (z, app)
    }

    #[test]
    fn the_frame_shows_the_worktrees_and_their_sessions() {
        let td = tempfile::TempDir::new().unwrap();
        let (z, app) = loaded(&td);
        let text = frame(&app, &z, 120, 20);
        assert!(text.contains("main [root]"));
        assert!(text.contains("rust"));
        assert!(text.contains("Conversion en Rust"));
        assert!(text.contains("Publier zrush sur GitHub"));
    }

    #[test]
    fn the_header_names_the_agent_and_counts_what_is_loaded() {
        let td = tempfile::TempDir::new().unwrap();
        let (z, app) = loaded(&td);
        let text = frame(&app, &z, 120, 20);
        assert!(text.contains("Agent"));
        assert!(text.contains("claude"));
        assert!(text.contains("1 live"));
        assert!(text.contains("1 resumable"));
    }

    #[test]
    fn the_title_counts_the_worktrees() {
        let td = tempfile::TempDir::new().unwrap();
        let (z, app) = loaded(&td);
        assert!(frame(&app, &z, 120, 20).contains("Worktrees(2)"));
    }

    #[test]
    fn a_narrow_terminal_drops_the_preview_rather_than_squashing_it() {
        let td = tempfile::TempDir::new().unwrap();
        let (z, app) = loaded(&td);
        assert!(frame(&app, &z, 120, 20).contains("Preview"));
        assert!(!frame(&app, &z, 80, 20).contains("Preview"));
    }

    #[test]
    fn a_modal_covers_the_list_rather_than_sitting_beside_it() {
        let td = tempfile::TempDir::new().unwrap();
        let (z, mut app) = loaded(&td);
        app.modal = Some(crate::ui::modal::Modal::Help);
        let text = frame(&app, &z, 120, 24);
        assert!(text.contains("Keys"));
        assert!(text.contains("ctrl-p"));
    }

    #[test]
    fn purge_mode_says_so_in_the_title_and_the_status_line() {
        let td = tempfile::TempDir::new().unwrap();
        let (z, mut app) = loaded(&td);
        app.mode = crate::ui::app::Mode::Purge;
        app.marked.insert(0);
        let text = frame(&app, &z, 120, 20);
        assert!(text.contains("Purge — 1 marked"));
        assert!(text.contains("space marks"));
    }

    #[test]
    fn the_filter_shows_what_is_being_typed() {
        let td = tempfile::TempDir::new().unwrap();
        let (z, mut app) = loaded(&td);
        app.mode = crate::ui::app::Mode::Filter;
        app.filter = "rust".into();
        assert!(frame(&app, &z, 120, 20).contains("/rust"));
    }

    #[test]
    fn a_flash_message_reaches_the_status_line() {
        let td = tempfile::TempDir::new().unwrap();
        let (z, mut app) = loaded(&td);
        app.flash("created feature");
        assert!(frame(&app, &z, 120, 20).contains("created feature"));
    }

    #[test]
    fn before_the_scan_lands_the_badge_is_the_cheap_disk_count() {
        let td = tempfile::TempDir::new().unwrap();
        let (z, mut app) = loaded(&td);
        app.scanned = false;
        app.resumable.clear();
        app.history.insert(z.main_root().to_path_buf(), 12);
        rebuild(&mut app, &z);
        let text = frame(&app, &z, 120, 20);
        assert!(text.contains("12 resumable"), "the immediate number");
        assert!(text.contains("loading"), "and it says it is still arriving");
    }

    #[test]
    fn once_scanned_the_badge_counts_what_was_actually_assigned() {
        // Otherwise the badge and the rows can disagree for good, and no
        // amount of pressing [...more] reconciles them.
        let td = tempfile::TempDir::new().unwrap();
        let (z, mut app) = loaded(&td);
        app.history.insert(z.main_root().to_path_buf(), 12);
        app.scanned = true;
        rebuild(&mut app, &z);
        let text = frame(&app, &z, 120, 20);
        assert!(!text.contains("12 resumable"));
    }

    #[test]
    fn a_tiny_terminal_draws_without_panicking() {
        let td = tempfile::TempDir::new().unwrap();
        let (z, app) = loaded(&td);
        for (w, h) in [(20_u16, 6_u16), (40, 10), (200, 60)] {
            let _ = frame(&app, &z, w, h);
        }
    }

    #[test]
    fn a_preview_that_has_not_arrived_yet_leaves_the_pane_empty() {
        // It is fetched on a worker; the frame must not wait for it.
        let td = tempfile::TempDir::new().unwrap();
        let (z, app) = loaded(&td);
        assert!(frame(&app, &z, 140, 24).contains("Preview"));
    }

    #[test]
    fn a_fetched_preview_is_drawn() {
        let td = tempfile::TempDir::new().unwrap();
        let (z, mut app) = loaded(&td);
        let key = App::preview_key(app.current().unwrap()).unwrap();
        app.previews
            .insert(key, Preview::Worktree(vec!["## main...origin/main".into()]));
        assert!(frame(&app, &z, 140, 24).contains("origin/main"));
    }

    #[test]
    fn a_row_is_only_asked_for_once() {
        let td = tempfile::TempDir::new().unwrap();
        let (z, mut app) = loaded(&td);
        let (tx, _rx) = mpsc::channel();
        let probes = crate::probe::Probes::new(tx);
        want_preview(&mut app, &z, &probes);
        assert_eq!(app.preview_pending.len(), 1);
        want_preview(&mut app, &z, &probes);
        assert_eq!(app.preview_pending.len(), 1, "asked twice for the same row");
    }

    #[test]
    fn a_worktree_preview_is_actually_fetched() {
        let td = tempfile::TempDir::new().unwrap();
        let (z, mut app) = loaded(&td);
        let (tx, rx) = mpsc::channel();
        let probes = crate::probe::Probes::new(tx);
        want_preview(&mut app, &z, &probes);
        drop(probes);
        let events: Vec<_> = rx.iter().collect();
        let found = events.iter().any(|e| {
            matches!(e, crate::probe::Event::Preview(_, _, Preview::Worktree(lines))
                if lines.iter().any(|l| l.contains("init")))
        });
        assert!(found, "the git log never came back: {events:?}");
    }
}
