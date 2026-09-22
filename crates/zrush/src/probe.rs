//! Getting the slow answers without making the user wait for them.
//!
//! The bash version blocked on `claude agents`, then on one `git status` per
//! worktree, then on the transcript scan, before it drew anything. Here the
//! worktree list is the only thing on the critical path: everything else
//! arrives as it lands, and the table fills in.
//!
//! One `mpsc::Receiver` carries the lot — key presses and probe results
//! alike — so the event loop has a single place to block.
//!
//! Every job carries the generation it was started for. A reload bumps the
//! generation, and results from a superseded run are dropped rather than
//! painted over the new ones.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;

use zrush_core::app::Zrush;
use zrush_core::model::{GitStatus, Session, Worktree};

/// How many worktrees are measured at once. A repo with fifty worktrees
/// must not start fifty threads to run fifty `git status`.
const STATUS_WORKERS: usize = 8;

#[derive(Debug)]
pub enum Event {
    Key(crossterm::event::KeyEvent),
    Resize(u16, u16),
    /// The only result the first frame waits for.
    Worktrees(u64, Vec<Worktree>),
    Status(u64, PathBuf, GitStatus),
    /// The cheap count, from one `readdir`: a number to show immediately.
    /// `Resumable` replaces it with what was actually assigned.
    History(u64, PathBuf, usize),
    Live(u64, Vec<Session>),
    Resumable(u64, Vec<Session>),
    /// What the preview pane should draw for one row, keyed the way the
    /// interface asked for it.
    Preview(u64, String, crate::ui::preview::Preview),
    /// A probe gave up. Not fatal: the row simply has no badge.
    Failed(u64, String),
}

impl Event {
    /// The generation this result belongs to, or `None` for input, which is
    /// never stale.
    pub fn generation(&self) -> Option<u64> {
        match self {
            Self::Key(_) | Self::Resize(..) => None,
            Self::Worktrees(g, _)
            | Self::Status(g, ..)
            | Self::History(g, ..)
            | Self::Live(g, _)
            | Self::Resumable(g, _)
            | Self::Preview(g, ..)
            | Self::Failed(g, _) => Some(*g),
        }
    }
}

pub struct Probes {
    tx: Sender<Event>,
    generation: Arc<AtomicU64>,
}

impl Probes {
    pub fn new(tx: Sender<Event>) -> Self {
        Self {
            tx,
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// Read the keyboard forever, on its own thread, so nothing the probes
    /// do can delay a key press.
    pub fn spawn_input(&self) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            loop {
                match crossterm::event::read() {
                    Ok(crossterm::event::Event::Key(k)) => {
                        if tx.send(Event::Key(k)).is_err() {
                            return;
                        }
                    }
                    Ok(crossterm::event::Event::Resize(w, h)) => {
                        if tx.send(Event::Resize(w, h)).is_err() {
                            return;
                        }
                    }
                    Ok(_) => {}
                    // The terminal is gone; so is the reason to read it.
                    Err(_) => return,
                }
            }
        });
    }

    /// Start a fresh round of probes and return its generation.
    ///
    /// `uncapped` lifts the transcript scan's bound, which the interface
    /// asks for once the user wants to see past the newest few.
    pub fn refresh(&self, z: &Arc<Zrush>, uncapped: bool) -> u64 {
        let round = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let tx = self.tx.clone();
        let z = Arc::clone(z);

        std::thread::spawn(move || {
            let worktrees = match z.worktrees() {
                Ok(w) => w,
                Err(e) => {
                    let _ = tx.send(Event::Failed(round, e.to_string()));
                    return;
                }
            };
            if tx.send(Event::Worktrees(round, worktrees.clone())).is_err() {
                return;
            }

            // The per-worktree numbers fan out; neither depends on the other.
            spawn_status(&z, &tx, round, &worktrees);

            // These two are sequential because they have to be: the scan
            // must know which sessions are already running, so it does not
            // list them twice.
            let live = z.live_sessions();
            if tx.send(Event::Live(round, live.clone())).is_err() {
                return;
            }
            let resumable = z.resumable_sessions(&worktrees, &live, uncapped);
            let _ = tx.send(Event::Resumable(round, resumable));
        });

        round
    }
}

/// What the preview pane draws, fetched off the drawing thread.
///
/// A worktree preview costs two `git` processes. Running them while the
/// frame is being built would mean every cursor move waits on git, which is
/// the very thing this rewrite set out to remove.
pub enum PreviewJob {
    Worktree(PathBuf),
    Conversation { agent: String, id: String },
}

/// Recent commits, with who wrote each one: on a shared branch the author
/// is half of what the log tells you. `%<(14,trunc)` keeps the column
/// straight whatever the name.
const LOG: &[&str] = &["log", "-n", "10", "--pretty=format:%h %<(14,trunc)%an %s%d"];

impl Probes {
    pub fn request_preview(&self, z: &Arc<Zrush>, key: String, job: PreviewJob) {
        let round = self.generation();
        let tx = self.tx.clone();
        let z = Arc::clone(z);
        std::thread::spawn(move || {
            let preview = match job {
                PreviewJob::Worktree(path) => {
                    let mut lines: Vec<String> = Vec::new();
                    if let Ok(out) = zrush_core::git::run(&path, &["status", "--short", "--branch"])
                    {
                        lines.extend(out.lines().take(15).map(str::to_string));
                    }
                    lines.push(String::new());
                    if let Ok(out) = zrush_core::git::run(&path, LOG) {
                        lines.extend(out.lines().map(str::to_string));
                    }
                    crate::ui::preview::Preview::Worktree(lines)
                }
                PreviewJob::Conversation { agent, id } => {
                    crate::ui::preview::Preview::Conversation(z.preview(&agent, &id))
                }
            };
            let _ = tx.send(Event::Preview(round, key, preview));
        });
    }
}

fn spawn_status(z: &Arc<Zrush>, tx: &Sender<Event>, round: u64, worktrees: &[Worktree]) {
    let want_status = z.config().status;
    let chunks = worktrees.len().div_ceil(STATUS_WORKERS.max(1)).max(1);
    for chunk in worktrees.chunks(chunks) {
        let paths: Vec<PathBuf> = chunk.iter().map(|w| w.path.clone()).collect();
        let tx = tx.clone();
        let z = Arc::clone(z);
        std::thread::spawn(move || {
            for p in paths {
                // One readdir: a number to show while the scan runs.
                let n = z.history_count(&p);
                if tx.send(Event::History(round, p.clone(), n)).is_err() {
                    return;
                }
                if want_status
                    && let Ok(s) = z.status(&p)
                    && tx.send(Event::Status(round, p, s)).is_err()
                {
                    return;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;
    use crate::testing::{git, scratch_repo, zrush};

    /// Everything a round produced. Dropping the `Probes` drops the only
    /// sender the test holds, so the channel closes as soon as the workers
    /// are done: no sleeping, no timeout, no flake.
    fn drain(p: Probes, rx: &mpsc::Receiver<Event>) -> Vec<Event> {
        drop(p);
        rx.iter().collect()
    }

    #[test]
    fn the_worktree_list_is_the_first_thing_to_land() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch_repo(&td));
        let (tx, rx) = mpsc::channel();
        let p = Probes::new(tx);
        p.refresh(&z, false);

        let first = rx.recv_timeout(Duration::from_secs(30)).unwrap();
        assert!(matches!(first, Event::Worktrees(_, ref w) if w.len() == 1));
    }

    #[test]
    fn every_worktree_reports_a_history_count_and_a_status() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        git(
            &repo,
            &["worktree", "add", "-q", ".claude/worktrees/a", "-b", "a"],
        );
        let z = zrush(&td, repo);
        let (tx, rx) = mpsc::channel();
        let p = Probes::new(tx);
        p.refresh(&z, false);

        let events = drain(p, &rx);
        let history = events
            .iter()
            .filter(|e| matches!(e, Event::History(..)))
            .count();
        let status = events
            .iter()
            .filter(|e| matches!(e, Event::Status(..)))
            .count();
        assert_eq!(history, 2, "one per worktree");
        assert_eq!(status, 2, "one per worktree");
    }

    #[test]
    fn the_status_probe_is_skipped_when_the_config_says_so() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let z = Arc::new(
            Zrush::new(
                zrush_core::config::dirs_under(td.path()),
                zrush_core::config::Config {
                    status: false,
                    ..zrush_core::config::Config::default()
                },
                repo,
                zrush_core::agent::all(),
                Box::new(zrush_core::host::RecordingHost::default()),
                None,
            )
            .unwrap(),
        );
        let (tx, rx) = mpsc::channel();
        let p = Probes::new(tx);
        p.refresh(&z, false);

        let events = drain(p, &rx);
        assert!(!events.iter().any(|e| matches!(e, Event::Status(..))));
        assert!(events.iter().any(|e| matches!(e, Event::History(..))));
    }

    #[test]
    fn live_and_resumable_both_arrive() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch_repo(&td));
        let (tx, rx) = mpsc::channel();
        let p = Probes::new(tx);
        p.refresh(&z, false);

        let events = drain(p, &rx);
        assert!(events.iter().any(|e| matches!(e, Event::Live(..))));
        assert!(events.iter().any(|e| matches!(e, Event::Resumable(..))));
    }

    #[test]
    fn each_refresh_gets_its_own_generation() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch_repo(&td));
        let (tx, _rx) = mpsc::channel();
        let p = Probes::new(tx);
        assert_eq!(p.refresh(&z, false), 1);
        assert_eq!(p.refresh(&z, false), 2);
        assert_eq!(p.generation(), 2);
    }

    #[test]
    fn results_carry_the_generation_they_were_started_for() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch_repo(&td));
        let (tx, rx) = mpsc::channel();
        let p = Probes::new(tx);
        let round = p.refresh(&z, false);

        let events = drain(p, &rx);
        assert!(!events.is_empty());
        for e in &events {
            assert_eq!(e.generation(), Some(round));
        }
    }

    #[test]
    fn a_worktree_preview_says_who_wrote_each_commit() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let z = zrush(&td, repo.clone());
        let (tx, rx) = mpsc::channel();
        let p = Probes::new(tx);
        p.request_preview(&z, "w".into(), PreviewJob::Worktree(repo));

        let events = drain(p, &rx);
        let Some(Event::Preview(_, _, crate::ui::preview::Preview::Worktree(lines))) =
            events.into_iter().next()
        else {
            panic!("no worktree preview");
        };
        let log = lines.join("\n");
        assert!(log.contains("zrush tests"), "no author in:\n{log}");
        assert!(log.contains("init"), "no subject in:\n{log}");
    }

    #[test]
    fn input_is_never_stale() {
        let k = crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Esc);
        assert_eq!(Event::Key(k).generation(), None);
        assert_eq!(Event::Resize(80, 24).generation(), None);
    }

    #[test]
    fn a_probe_on_a_directory_that_is_not_a_repo_fails_without_panicking() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let z = zrush(&td, repo.clone());
        // Make git unable to answer: the worktree list now points nowhere.
        std::fs::remove_dir_all(repo.join(".git")).unwrap();

        let (tx, rx) = mpsc::channel();
        let p = Probes::new(tx);
        p.refresh(&z, false);
        let events = drain(p, &rx);
        assert!(events.iter().any(|e| matches!(e, Event::Failed(..))));
    }
}
