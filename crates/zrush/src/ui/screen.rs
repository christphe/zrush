//! Putting the pieces on the terminal, and the loop that keeps them there.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::mpsc;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use zrush_core::app::Zrush;

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
        .snap
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
            worktrees: app.snap.worktrees.len(),
            live: app.snap.live.len(),
            resumable: app.snap.resumable.len(),
            loaded: app.snap.scanned,
            update: app.update.as_deref(),
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
        _ => format!("Worktrees({})", app.snap.worktrees.len()),
    };
    table::render(
        f,
        body[0],
        &table::View {
            rows: &visible,
            matched: &app.matched,
            home: app.home.as_deref(),
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

/// One frame per tick. Braille rather than ASCII: it turns in place
/// instead of jumping a column, and every terminal that draws the tree's
/// box characters has it.
const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

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
    // What is happening now outranks what happened last: a spinner that
    // the previous action's flash sits on top of is worse than no spinner.
    let right = match &app.busy {
        Some(what) => {
            let frame = SPINNER[app.spin % SPINNER.len()];
            format!("{frame} {what}…")
        }
        None => app.flash.clone().unwrap_or_default(),
    };
    let gap =
        (area.width as usize).saturating_sub(left.chars().count() + right.chars().count() + 2);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {left}"), theme::header_label()),
            Span::raw(" ".repeat(gap)),
            Span::styled(
                right,
                if app.busy.is_some() {
                    theme::header_key()
                } else {
                    theme::badge()
                },
            ),
        ])),
        area,
    );
}

/// Rebuild the rows from whatever has arrived so far.
pub fn rebuild(app: &mut App, z: &Zrush) {
    let rows = app.snap.rows(z, &app.tree_view());
    app.set_rows(rows);
}

/// What the preview pane should draw for the row under the cursor: whatever
/// has been fetched for it, and nothing while a worker is still on it.
pub fn preview_for(app: &App) -> Preview {
    app.current()
        .and_then(App::preview_key)
        .map_or(Preview::Empty, |k| app.snap.preview(&k))
}

/// Ask for the preview of the row under the cursor, once.
pub fn want_preview(app: &mut App, z: &Arc<Zrush>, probes: &zrush_core::loader::Loader) {
    let Some(row) = app.current().cloned() else {
        return;
    };
    if let Some((key, job)) = app.snap.want_preview(&row, &app.active_agent) {
        probes.request_preview(z, key, job);
    }
}

/// Apply one event. Returns false when the interface should stop.
pub fn apply(
    app: &mut App,
    z: &Arc<Zrush>,
    probes: &zrush_core::loader::Loader,
    msg: crate::probe::Msg,
) -> bool {
    use crate::probe::Msg;
    use zrush_core::snapshot::Applied;

    // A result from a superseded reload must not paint over the new one.
    if msg.generation().is_some_and(|g| g != probes.generation()) {
        return true;
    }

    let event = match msg {
        Msg::Key(k) => return handle(app, z, probes, k),
        Msg::Resize(..) => return true,
        Msg::Core(e) => e,
    };
    match app.snap.apply(event, probes.generation()) {
        Applied::Stale | Applied::Preview | Applied::Nothing => {}
        Applied::Rows => rebuild(app, z),
        Applied::Acted(done) => {
            app.busy = None;
            match done.result {
                Ok(msg) => {
                    app.flash(msg);
                    probes.refresh(z, app.show_all);
                }
                Err(e) => app.error(done.label, e),
            }
        }
        Applied::Failed(e) => app.flash(e),
        Applied::Update(tag) => app.update = Some(tag),
    }
    true
}

fn handle(
    app: &mut App,
    z: &Arc<Zrush>,
    probes: &zrush_core::loader::Loader,
    key: crossterm::event::KeyEvent,
) -> bool {
    app.flash = None;
    let mut settings_error: Option<zrush_core::error::ZrushError> = None;
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
            report(app, "Open", z.open(&worktree, bind));
        }
        Action::RunAgent { worktree, session } => run_agent(app, z, &worktree, session.as_ref()),
        Action::CreateWorktree {
            name,
            kind,
            at,
            hand_over,
        } => create(app, z, probes, name, kind, at, hand_over),
        Action::AdoptSession {
            agent,
            id,
            worktree,
        } => {
            let z2 = Arc::clone(z);
            let w = worktree.clone();
            start(app, probes, "Resume this session", move || {
                z2.adopt_session(&agent, &id, &w)?;
                // Some, not None: None means "a fresh conversation here" and
                // clears the association adopt_session just wrote, which is
                // the whole point of adopting it.
                z2.open(&w, Some((&agent, &id)))?;
                Ok(format!("moved to {}", w.display()))
            });
        }
        Action::DeleteSession {
            agent,
            id,
            worktree,
        } => {
            let z = Arc::clone(z);
            start(app, probes, "Delete session", move || {
                let n = z.delete_session(&agent, &id, false, worktree.as_deref())?;
                Ok(format!("deleted {n} file(s)"))
            });
        }
        Action::RemoveWorktree {
            path,
            with_sessions,
        } => remove(app, z, probes, path, with_sessions),
        Action::Purge {
            worktrees,
            sessions,
        } => purge(app, z, probes, worktrees, sessions),
        Action::SwitchAgent(id) => switch_agent(app, z, probes, id),
        Action::SetEditor(name) => set_editor(app, z, &z.set_editor(&name), &name),
        Action::SetEditorCommand(cmd) => {
            let r = z.set_editor_command(&cmd);
            set_editor(app, z, &r, &cmd.join(" "));
        }
        Action::SetSetting { key, value } => {
            let r = z.edit_config(|c| {
                // The error is kept for the caller: a bad value must not
                // be written, and must not be silently dropped either.
                settings_error = c.set(&key, &value).err();
            });
            settings_done(app, z, probes, &key, settings_error.map_or(r, Err));
        }
        Action::ToggleSetting(key) => {
            let r = z.edit_config(|c| settings_error = c.toggle(&key).err());
            settings_done(app, z, probes, &key, settings_error.map_or(r, Err));
        }
        // The installer needs the terminal: it is run once the interface
        // has let go of it, by whoever called `run`.
        Action::Update(tag) => {
            app.install = Some(tag);
            return false;
        }
        Action::SkipUpdate(tag) => match zrush_core::update::skip(z.dirs(), &tag) {
            Ok(()) => app.flash(format!("{tag} skipped; the next release will show")),
            Err(e) => app.error("Skip this version", e),
        },
    }
    true
}

fn report(app: &mut App, what: &str, r: zrush_core::error::Result<()>) {
    if let Err(e) = r {
        app.error(what, e);
    }
}

fn run_agent(
    app: &mut App,
    z: &Arc<Zrush>,
    worktree: &std::path::Path,
    session: Option<&(String, String)>,
) {
    // The session's own agent outranks the one being listed: a worktree
    // bound under another agent still resumes correctly.
    let listed = z.active_agent().map(zrush_core::agent::Agent::id);
    let Some(agent) = session.map(|(a, _)| a.as_str()).or(listed) else {
        app.flash("no agent installed");
        return;
    };
    let resume = session.map(|(_, i)| i.as_str());
    report(app, "New session", z.run_agent(worktree, agent, resume));
}

/// Hand a long action to a worker and put the spinner up. A second one is
/// refused rather than queued: two `git worktree` commands at once on the
/// same repository is not something to find out about afterwards.
fn start<F>(app: &mut App, probes: &zrush_core::loader::Loader, label: &str, f: F)
where
    F: FnOnce() -> zrush_core::error::Result<String> + Send + 'static,
{
    if let Some(busy) = &app.busy {
        app.flash(format!("still {busy}"));
        return;
    }
    app.busy = Some(label.to_lowercase());
    app.spin = 0;
    probes.spawn_action(label, f);
}

fn create(
    app: &mut App,
    z: &Arc<Zrush>,
    probes: &zrush_core::loader::Loader,
    name: String,
    kind: zrush_core::app::BranchKind,
    at: Option<std::path::PathBuf>,
    hand_over: Option<(String, String)>,
) {
    let z = Arc::clone(z);
    start(app, probes, "New worktree", move || {
        let dest = z.create_worktree_at(at.as_deref(), &name, kind)?;
        let bind = hand_over.as_ref().map(|(a, i)| (a.as_str(), i.as_str()));
        z.open(&dest, bind)?;
        Ok(format!("created {name}"))
    });
}

fn remove(
    app: &mut App,
    z: &Arc<Zrush>,
    probes: &zrush_core::loader::Loader,
    path: std::path::PathBuf,
    with_sessions: bool,
) {
    let mine = app.snap.sessions_in(&path);
    let z = Arc::clone(z);
    start(app, probes, "Remove worktree", move || {
        let r = z.remove_worktree(&path, &mine, with_sessions)?;
        Ok(format!(
            "removed, {} transcript(s) deleted, {} running kept",
            r.transcripts, r.running_kept
        ))
    });
}

/// Each item is independent: a dirty worktree refuses without stopping the
/// rest, and only the closing summary reaches the status line.
fn purge(
    app: &mut App,
    z: &Arc<Zrush>,
    probes: &zrush_core::loader::Loader,
    worktrees: Vec<std::path::PathBuf>,
    sessions: Vec<(String, String)>,
) {
    let z = Arc::clone(z);
    start(app, probes, "Purge", move || {
        let mut removed = 0;
        let mut refused = 0;
        for p in &worktrees {
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
        Ok(format!(
            "purged {removed} worktree(s), {gone} transcript(s){kept}"
        ))
    });
}

/// A setting was written, or was not. Either way the interface goes back
/// to the screen the change was asked from, with what the file now says —
/// nothing here trusts its own idea of the new value.
fn settings_done(
    app: &mut App,
    z: &Arc<Zrush>,
    probes: &zrush_core::loader::Loader,
    key: &str,
    r: zrush_core::error::Result<zrush_core::config::Config>,
) {
    match r {
        Ok(cfg) => {
            adopt(app, &cfg);
            app.flash(format!("{key} = {}", short(&cfg.show(key))));
            probes.refresh(z, app.show_all);
        }
        Err(e) => {
            // The error box IS the modal: reopening the settings screen here
            // would replace it, which is how a bad value used to look like
            // nothing happening.
            app.error("Settings", e);
            return;
        }
    }
    if app.settings_open {
        app.open_settings();
    }
}

/// What the interface keeps its own copy of, refreshed from the file.
fn adopt(app: &mut App, cfg: &zrush_core::config::Config) {
    app.more_step = cfg.more_step;
    app.resumable_max = cfg.resumable_max;
    app.cfg = cfg.clone();
}

fn short(v: &str) -> String {
    if v.is_empty() {
        "(unset)".into()
    } else {
        v.to_string()
    }
}

/// The host has already been told; what is left is the interface's own
/// copy of the name, and saying so. A config that could not be written is
/// an error even though the change took: the next run would forget it.
fn set_editor(app: &mut App, z: &Arc<Zrush>, saved: &zrush_core::error::Result<()>, label: &str) {
    app.editor = z.editor_label();
    adopt(app, &z.config());
    match saved {
        Ok(()) => app.flash(format!("editor: {label}")),
        Err(e) => app.error("Editor", e),
    }
    // Picked from the settings screen: go back to it, not to the list.
    if app.settings_open {
        app.open_settings();
    }
}

fn switch_agent(app: &mut App, z: &Arc<Zrush>, probes: &zrush_core::loader::Loader, id: String) {
    // Switching is what the interface lists; remembering it is what the
    // next run starts on. It used to do only the first, so the choice was
    // forgotten every time.
    match z
        .set_active_agent(&id)
        .and_then(|()| z.edit_config(|c| c.agent = Some(id.clone())))
    {
        Ok(cfg) => {
            adopt(app, &cfg);
            app.active_agent = id;
            // Everything listed belonged to the agent we just left.
            app.snap.live.clear();
            app.snap.resumable.clear();
            app.snap.scanned = false;
            rebuild(app, z);
            probes.refresh(z, app.show_all);
            if app.settings_open {
                app.open_settings();
            }
        }
        Err(e) => app.error("Switch agent", e),
    }
}

/// How often the spinner turns while a worker is out. Fast enough to read
/// as motion, slow enough that a long action is not a busy loop.
const SPIN_EVERY: std::time::Duration = std::time::Duration::from_millis(90);

/// Run the interface until it is told to stop. What comes back is the
/// release to install, when that is why it stopped.
pub fn run(
    mut app: App,
    z: &Arc<Zrush>,
    check_updates: bool,
) -> zrush_core::error::Result<Option<String>> {
    let mut term = ratatui::init();
    let (tx, rx) = mpsc::channel();
    let probes = crate::probe::loader(tx.clone());
    crate::probe::spawn_input(tx);
    probes.refresh(z, false);
    if check_updates {
        probes.spawn_update_check(z.dirs().clone());
    }

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

        // Blocking recv while idle: a terminal that redraws for nothing
        // costs battery on a tool that sits open all day. The timeout
        // exists only to turn the spinner.
        let event = if app.busy.is_some() {
            match rx.recv_timeout(SPIN_EVERY) {
                Ok(e) => Some(e),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    app.spin = app.spin.wrapping_add(1);
                    None
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break Ok(()),
            }
        } else {
            match rx.recv() {
                Ok(e) => Some(e),
                Err(_) => break Ok(()),
            }
        };
        if let Some(event) = event
            && !apply(&mut app, z, &probes, event)
        {
            break Ok(());
        }
    };
    ratatui::restore();
    out.map(|()| app.install)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    fn key(code: crossterm::event::KeyCode) -> crossterm::event::KeyEvent {
        crossterm::event::KeyEvent::from(code)
    }
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

    /// The whole sequence, through the keyboard: adopting an orphan must
    /// leave it bound. It used to refile the transcript and then clear the
    /// association on the way out, so the panel opened a fresh
    /// conversation — the symptom that started this.
    #[test]
    fn adopting_an_orphan_leaves_it_bound() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let z = zrush(&td, repo.clone());
        let id = "cccccccc-1111-2222-3333-444455556666";

        // Its transcript is filed under a worktree that no longer exists.
        let gone = repo.join(".claude/worktrees/gone");
        let dir = zrush_core::config::dirs_under(td.path())
            .claude_projects
            .join(zrush_core::agent::claude::projects::slugify(&gone));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{id}.jsonl")), "{}\n").unwrap();

        let mut app = App::new(20, 5);
        app.active_agent = "claude".into();
        app.snap.worktrees = z.worktrees().unwrap();
        app.snap.resumable = vec![session(
            id,
            "orphan",
            gone.to_str().unwrap(),
            SessionKind::Resumable,
        )];
        app.snap.scanned = true;
        rebuild(&mut app, &z);
        // Onto the orphan row.
        app.cursor = app
            .visible
            .iter()
            .position(|&i| app.rows[i].session_id.as_deref() == Some(id))
            .unwrap();

        let (tx, rx) = mpsc::channel();
        let probes = crate::probe::loader(tx);
        handle(&mut app, &z, &probes, key(crossterm::event::KeyCode::Enter));
        handle(&mut app, &z, &probes, key(crossterm::event::KeyCode::Down));
        handle(&mut app, &z, &probes, key(crossterm::event::KeyCode::Enter));

        // The worker answers; nothing else is listening.
        let done = rx.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
        match done {
            crate::probe::Msg::Core(zrush_core::loader::Event::Acted(d)) => d.result.unwrap(),
            other => panic!("expected the action to finish: {other:?}"),
        };

        let bound = z.binding(&repo).expect("the association must survive");
        assert_eq!(bound.id, id);
        let moved = zrush_core::config::dirs_under(td.path())
            .claude_projects
            .join(zrush_core::agent::claude::projects::slugify(&repo))
            .join(format!("{id}.jsonl"));
        assert!(moved.is_file(), "and the transcript must have moved");
    }

    /// A value the config refuses must be seen. The error box used to be
    /// posted and then replaced by the settings screen reopening over it,
    /// so a typo looked like nothing happening.
    #[test]
    fn a_refused_setting_shows_its_error_instead_of_the_screen() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch_repo(&td));
        let (tx, _rx) = mpsc::channel();
        let probes = crate::probe::loader(tx);
        let mut app = App::new(20, 5);
        app.cfg = z.config();

        app.open_settings();
        assert!(app.settings_open);
        // Down to resumable_max, then type something that is not a number.
        for _ in 0..6 {
            handle(&mut app, &z, &probes, key(crossterm::event::KeyCode::Down));
        }
        handle(&mut app, &z, &probes, key(crossterm::event::KeyCode::Enter));
        handle(
            &mut app,
            &z,
            &probes,
            key(crossterm::event::KeyCode::Backspace),
        );
        for c in "lots".chars() {
            handle(
                &mut app,
                &z,
                &probes,
                key(crossterm::event::KeyCode::Char(c)),
            );
        }
        handle(&mut app, &z, &probes, key(crossterm::event::KeyCode::Enter));

        match &app.modal {
            Some(crate::ui::modal::Modal::Error { body, .. }) => {
                assert!(body.contains("not a number"), "{body}");
            }
            other => panic!("the error was replaced by {other:?}"),
        }
        // And the old value is still what the config says.
        assert_eq!(z.config().resumable_max, 5);
    }

    /// The case that cost an afternoon: a session started at the repo root
    /// and moved into a worktree is listed under that worktree, but filed
    /// under the root. Resuming from the row opened an empty panel and
    /// said nothing.
    #[test]
    fn a_session_filed_somewhere_else_says_so_on_its_row() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        git(
            &repo,
            &["worktree", "add", "-q", ".claude/worktrees/w", "-b", "w"],
        );
        let z = zrush(&td, repo.clone());
        let projects = zrush_core::config::dirs_under(td.path()).claude_projects;
        let here = "aaaaaaaa-1111-2222-3333-444455556666";
        let moved = "bbbbbbbb-1111-2222-3333-444455556666";
        let wt = repo.join(".claude/worktrees/w");

        // One filed under the worktree it works in, one filed at the root.
        for (dir, id) in [(wt.clone(), here), (repo.clone(), moved)] {
            let d = projects.join(zrush_core::agent::claude::projects::slugify(&dir));
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join(format!("{id}.jsonl")), "{}\n").unwrap();
        }

        let mut app = App::new(20, 5);
        app.active_agent = "claude".into();
        app.snap.worktrees = z.worktrees().unwrap();
        app.snap.resumable = vec![
            session(
                here,
                "filed right",
                wt.to_str().unwrap(),
                SessionKind::Resumable,
            ),
            session(
                moved,
                "moved here",
                wt.to_str().unwrap(),
                SessionKind::Resumable,
            ),
        ];
        app.snap.scanned = true;
        rebuild(&mut app, &z);

        let line = |id: &str| {
            app.rows
                .iter()
                .find(|r| r.session_id.as_deref() == Some(id))
                .map(|r| r.status.clone())
                .unwrap()
        };
        assert!(
            line(moved).contains("filed elsewhere"),
            "the moved one must say so: {}",
            line(moved)
        );
        assert!(
            !line(here).contains("filed elsewhere"),
            "the other must not: {}",
            line(here)
        );
    }

    /// No transcript at all is a different thing, and not worth a warning
    /// on every row — which is what marking it produced.
    #[test]
    fn a_session_with_no_transcript_anywhere_is_not_called_misfiled() {
        let td = tempfile::TempDir::new().unwrap();
        let (z, app) = loaded(&td);
        assert!(
            !app.rows
                .iter()
                .any(|r| r.status.contains("filed elsewhere")),
            "nothing on disk, nothing to say"
        );
        drop(z);
    }

    #[test]
    fn a_long_action_says_it_is_working_instead_of_freezing() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch_repo(&td));
        let mut app = App::new(20, 5);
        app.busy = Some("remove worktree".into());
        app.flash("created feat");
        let text = frame(&app, &z, 100, 14);
        assert!(text.contains("remove worktree…"), "no spinner in:\n{text}");
        // The spinner replaces the last action's line rather than sharing
        // the row with it.
        assert!(!text.contains("created feat"));
        assert!(
            SPINNER.iter().any(|c| text.contains(*c)),
            "no spinner frame in:\n{text}"
        );
    }

    #[test]
    fn a_second_long_action_is_refused_while_one_runs() {
        let (tx, _rx) = mpsc::channel();
        let probes = crate::probe::loader(tx);
        let mut app = App::new(20, 5);
        app.busy = Some("remove worktree".into());

        start(&mut app, &probes, "New worktree", || Ok(String::new()));

        assert_eq!(app.busy.as_deref(), Some("remove worktree"));
        assert!(app.flash.unwrap().contains("still remove worktree"));
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
        app.snap.scanned = false;
        app.snap.resumable.clear();
        app.snap.history.insert(z.main_root().to_path_buf(), 12);
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
        app.snap.history.insert(z.main_root().to_path_buf(), 12);
        app.snap.scanned = true;
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
        app.snap.apply(
            zrush_core::loader::Event::Preview(
                0,
                key,
                Preview::Worktree(vec!["## main...origin/main".into()]),
            ),
            0,
        );
        assert!(frame(&app, &z, 140, 24).contains("origin/main"));
    }

    #[test]
    fn a_row_is_only_asked_for_once() {
        let td = tempfile::TempDir::new().unwrap();
        let (z, mut app) = loaded(&td);
        let (tx, rx) = mpsc::channel();
        let probes = crate::probe::loader(tx);
        want_preview(&mut app, &z, &probes);
        want_preview(&mut app, &z, &probes);
        drop(probes);
        let fetched = rx.iter().count();
        assert_eq!(fetched, 1, "asked twice for the same row");
    }

    #[test]
    fn a_worktree_preview_is_actually_fetched() {
        let td = tempfile::TempDir::new().unwrap();
        let (z, mut app) = loaded(&td);
        let (tx, rx) = mpsc::channel();
        let probes = crate::probe::loader(tx);
        want_preview(&mut app, &z, &probes);
        drop(probes);
        let events: Vec<_> = rx.iter().collect();
        let found = events.iter().any(|e| {
            matches!(e, crate::probe::Msg::Core(zrush_core::loader::Event::Preview(_, _, Preview::Worktree(lines)))
                if lines.iter().any(|l| l.contains("init")))
        });
        assert!(found, "the git log never came back: {events:?}");
    }
}
