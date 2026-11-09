//! What a key or a loader result does to the screen, and the actions it
//! starts.

use super::{Action, App, Arc, Preview, Zrush, ZrushError, actions};

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

pub(super) fn handle(
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
            let z = Arc::clone(z);
            start(app, probes, "Resume this session", move || {
                actions::adopt(&z, &agent, &id, &worktree)
            });
        }
        Action::DeleteSession {
            agent,
            id,
            worktree,
        } => {
            let z = Arc::clone(z);
            start(app, probes, "Delete session", move || {
                actions::delete_session(&z, &agent, &id, worktree.as_deref())
            });
        }
        Action::RemoveWorktree {
            path,
            with_sessions,
        } => remove(app, z, probes, path, with_sessions),
        Action::Purge(plan) => purge(app, z, probes, plan),
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

pub(super) fn report(app: &mut App, what: &str, r: zrush_core::error::Result<()>) {
    if let Err(e) = r {
        app.error(what, e);
    }
}

pub(super) fn run_agent(
    app: &mut App,
    z: &Arc<Zrush>,
    worktree: &std::path::Path,
    session: Option<&(String, String)>,
) {
    let session = session.map(|(a, i)| (a.as_str(), i.as_str()));
    match actions::run_agent(z, worktree, session) {
        // Nothing to start is a line, not an error box.
        Err(ZrushError::NoAgent) => app.flash("no agent installed"),
        r => report(app, "New session", r),
    }
}

/// Hand a long action to a worker and put the spinner up. A second one is
/// refused rather than queued: two `git worktree` commands at once on the
/// same repository is not something to find out about afterwards.
pub(super) fn start<F>(app: &mut App, probes: &zrush_core::loader::Loader, label: &str, f: F)
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

pub(super) fn create(
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
        let bind = hand_over.as_ref().map(|(a, i)| (a.as_str(), i.as_str()));
        actions::create(&z, &name, kind, at.as_deref(), bind)
    });
}

pub(super) fn remove(
    app: &mut App,
    z: &Arc<Zrush>,
    probes: &zrush_core::loader::Loader,
    path: std::path::PathBuf,
    with_sessions: bool,
) {
    let mine = app.snap.sessions_in(&path);
    let z = Arc::clone(z);
    start(app, probes, "Remove worktree", move || {
        actions::remove(&z, &path, &mine, with_sessions)
    });
}

/// Each item is independent: a dirty worktree refuses without stopping the
/// rest, and only the closing summary reaches the status line.
pub(super) fn purge(
    app: &mut App,
    z: &Arc<Zrush>,
    probes: &zrush_core::loader::Loader,
    plan: zrush_core::purge::Plan,
) {
    let z = Arc::clone(z);
    start(app, probes, "Purge", move || actions::purge(&z, &plan));
}

/// A setting was written, or was not. Either way the interface goes back
/// to the screen the change was asked from, with what the file now says —
/// nothing here trusts its own idea of the new value.
pub(super) fn settings_done(
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
pub(super) fn adopt(app: &mut App, cfg: &zrush_core::config::Config) {
    app.more_step = cfg.more_step;
    app.resumable_max = cfg.resumable_max;
    app.cfg = cfg.clone();
}

pub(super) fn short(v: &str) -> String {
    if v.is_empty() {
        "(unset)".into()
    } else {
        v.to_string()
    }
}

/// The host has already been told; what is left is the interface's own
/// copy of the name, and saying so. A config that could not be written is
/// an error even though the change took: the next run would forget it.
pub(super) fn set_editor(
    app: &mut App,
    z: &Arc<Zrush>,
    saved: &zrush_core::error::Result<()>,
    label: &str,
) {
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

pub(super) fn switch_agent(
    app: &mut App,
    z: &Arc<Zrush>,
    probes: &zrush_core::loader::Loader,
    id: String,
) {
    // Switching is what the interface lists; remembering it is what the
    // next run starts on. It used to do only the first, so the choice was
    // forgotten every time.
    match actions::switch_agent(z, &id) {
        Ok(cfg) => {
            adopt(app, &cfg);
            app.active_agent = id;
            // Everything listed belonged to the agent we just left.
            app.snap.forget_sessions();
            rebuild(app, z);
            probes.refresh(z, app.show_all);
            if app.settings_open {
                app.open_settings();
            }
        }
        Err(e) => app.error("Switch agent", e),
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::*;
    use super::*;
    use zrush_core::model::SessionKind;

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
