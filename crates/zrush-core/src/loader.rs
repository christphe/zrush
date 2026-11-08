//! Getting the slow answers without making the user wait for them.
//!
//! The bash version blocked on `claude agents`, then on one `git status` per
//! worktree, then on the transcript scan, before it drew anything. Here the
//! worktree list is the only thing on the critical path: everything else
//! arrives as it lands, and the table fills in.
//!
//! Results go to a `Sink`, which each interface wires to its own event
//! loop: the terminal one merges them with key presses on one channel, a
//! window wakes itself to repaint.
//!
//! Every job carries the generation it was started for. A reload bumps the
//! generation, and results from a superseded run are dropped rather than
//! painted over the new ones.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::agent::Turn;
use crate::app::Zrush;
use crate::model::{GitStatus, Session, Worktree};

/// How many worktrees are measured at once. A repo with fifty worktrees
/// must not start fifty threads to run fifty `git status`.
const STATUS_WORKERS: usize = 8;

/// Where results go. Returns `false` once nobody listens any more — the
/// window closed, the tab went away — and every worker stops there.
pub type Sink = Arc<dyn Fn(Event) -> bool + Send + Sync>;

/// What a preview pane shows for one row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preview {
    /// Nothing to show, or nothing fetched yet.
    Empty,
    /// Lines of `git status` and `git log`, already fetched.
    Worktree(Vec<String>),
    Conversation(Vec<Turn>),
}

#[derive(Debug)]
pub enum Event {
    /// The only result the first frame waits for.
    Worktrees(u64, Vec<Worktree>),
    Status(u64, PathBuf, GitStatus),
    /// The cheap count, from one `readdir`: a number to show immediately.
    /// `Resumable` replaces it with what was actually assigned.
    History(u64, PathBuf, usize),
    Live(u64, Vec<Session>),
    Resumable(u64, Vec<Session>),
    /// Branches a new worktree could be opened on, for the ctrl-w list.
    Branches(u64, Vec<String>),
    /// What the preview pane should draw for one row, keyed the way the
    /// interface asked for it.
    Preview(u64, String, Preview),
    /// A long action finished. Never stale: a refresh started while it ran
    /// must not throw its answer away.
    Acted(Done),
    /// A probe gave up. Not fatal: the row simply has no badge.
    Failed(u64, String),
    /// A newer release, by its tag. Never stale: a reload changes nothing
    /// about what was published.
    Update(String),
}

/// What a long action came back with: the line to show, or what went
/// wrong, under the title the interface put on the spinner.
#[derive(Debug)]
pub struct Done {
    pub label: String,
    pub result: crate::error::Result<String>,
}

impl Event {
    /// The generation this result belongs to, or `None` for what is never
    /// stale.
    pub fn generation(&self) -> Option<u64> {
        match self {
            Self::Acted(_) | Self::Update(_) => None,
            Self::Worktrees(g, _)
            | Self::Status(g, ..)
            | Self::History(g, ..)
            | Self::Live(g, _)
            | Self::Resumable(g, _)
            | Self::Preview(g, ..)
            | Self::Branches(g, _)
            | Self::Failed(g, _) => Some(*g),
        }
    }
}

pub struct Loader {
    sink: Sink,
    generation: Arc<AtomicU64>,
}

impl Loader {
    pub fn new(sink: Sink) -> Self {
        Self {
            sink,
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// Run something slow off the drawing thread, so the interface can say
    /// it is working instead of freezing mid-frame.
    pub fn spawn_action<F>(&self, label: &str, f: F)
    where
        F: FnOnce() -> crate::error::Result<String> + Send + 'static,
    {
        let sink = Arc::clone(&self.sink);
        let label = label.to_string();
        std::thread::spawn(move || {
            let result = f();
            let _ = sink(Event::Acted(Done { label, result }));
        });
    }

    /// Ask whether a newer release exists, once, without holding anything
    /// up. Silent unless there is one to offer.
    pub fn spawn_update_check(&self, dirs: crate::config::Dirs) {
        let sink = Arc::clone(&self.sink);
        std::thread::spawn(move || {
            use crate::update;
            let url = update::latest_url();
            if let Some(tag) = update::check(&dirs, update::current(), update::now(), || {
                update::fetch_latest(&url)
            }) {
                let _ = sink(Event::Update(tag));
            }
        });
    }

    /// Start a fresh round of probes and return its generation.
    ///
    /// `uncapped` lifts the transcript scan's bound, which the interface
    /// asks for once the user wants to see past the newest few.
    pub fn refresh(&self, z: &Arc<Zrush>, uncapped: bool) -> u64 {
        let round = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let sink = Arc::clone(&self.sink);
        let z = Arc::clone(z);

        std::thread::spawn(move || {
            let worktrees = match z.worktrees() {
                Ok(w) => w,
                Err(e) => {
                    let _ = sink(Event::Failed(round, e.to_string()));
                    return;
                }
            };
            if !sink(Event::Worktrees(round, worktrees.clone())) {
                return;
            }

            // The per-worktree numbers fan out; neither depends on the other.
            spawn_status(&z, &sink, round, &worktrees);
            // Two `for-each-ref` calls, so it rides along here rather than
            // making ctrl-w wait for git.
            let _ = sink(Event::Branches(round, z.openable_branches()));

            // These two are sequential because they have to be: the scan
            // must know which sessions are already running, so it does not
            // list them twice.
            let live = z.live_sessions();
            if !sink(Event::Live(round, live.clone())) {
                return;
            }
            let resumable = z.resumable_sessions(&worktrees, &live, uncapped);
            let _ = sink(Event::Resumable(round, resumable));
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

impl Loader {
    pub fn request_preview(&self, z: &Arc<Zrush>, key: String, job: PreviewJob) {
        let round = self.generation();
        let sink = Arc::clone(&self.sink);
        let z = Arc::clone(z);
        std::thread::spawn(move || {
            let preview = match job {
                PreviewJob::Worktree(path) => {
                    let mut lines: Vec<String> = Vec::new();
                    if let Ok(out) = crate::git::run(&path, &["status", "--short", "--branch"]) {
                        lines.extend(out.lines().take(15).map(str::to_string));
                    }
                    lines.push(String::new());
                    if let Ok(out) = crate::git::run(&path, LOG) {
                        lines.extend(out.lines().map(str::to_string));
                    }
                    Preview::Worktree(lines)
                }
                PreviewJob::Conversation { agent, id } => {
                    Preview::Conversation(z.preview(&agent, &id))
                }
            };
            let _ = sink(Event::Preview(round, key, preview));
        });
    }
}

fn spawn_status(z: &Arc<Zrush>, sink: &Sink, round: u64, worktrees: &[Worktree]) {
    let want_status = z.config().status;
    let chunks = worktrees.len().div_ceil(STATUS_WORKERS.max(1)).max(1);
    for chunk in worktrees.chunks(chunks) {
        let paths: Vec<PathBuf> = chunk.iter().map(|w| w.path.clone()).collect();
        let sink = Arc::clone(sink);
        let z = Arc::clone(z);
        std::thread::spawn(move || {
            for p in paths {
                // One readdir: a number to show while the scan runs.
                let n = z.history_count(&p);
                if !sink(Event::History(round, p.clone(), n)) {
                    return;
                }
                if want_status
                    && let Ok(s) = z.status(&p)
                    && !sink(Event::Status(round, p, s))
                {
                    return;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;
    use crate::testing::{git, scratch_repo, zrush};

    /// A loader whose results land in a channel, as a test reads them.
    fn loader() -> (Loader, mpsc::Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        let sink: Sink = Arc::new(move |e| tx.send(e).is_ok());
        (Loader::new(sink), rx)
    }

    /// Everything a round produced. Dropping the `Probes` drops the only
    /// sender the test holds, so the channel closes as soon as the workers
    /// are done: no sleeping, no timeout, no flake.
    fn drain(p: Loader, rx: &mpsc::Receiver<Event>) -> Vec<Event> {
        drop(p);
        rx.iter().collect()
    }

    #[test]
    fn the_worktree_list_is_the_first_thing_to_land() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch_repo(&td));
        let (p, rx) = loader();
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
        let (p, rx) = loader();
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
                crate::config::dirs_under(td.path()),
                crate::config::Config {
                    status: false,
                    ..crate::config::Config::default()
                },
                repo,
                crate::agent::all(),
                Box::new(crate::host::RecordingHost::default()),
                None,
            )
            .unwrap(),
        );
        let (p, rx) = loader();
        p.refresh(&z, false);

        let events = drain(p, &rx);
        assert!(!events.iter().any(|e| matches!(e, Event::Status(..))));
        assert!(events.iter().any(|e| matches!(e, Event::History(..))));
    }

    #[test]
    fn live_and_resumable_both_arrive() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch_repo(&td));
        let (p, rx) = loader();
        p.refresh(&z, false);

        let events = drain(p, &rx);
        assert!(events.iter().any(|e| matches!(e, Event::Live(..))));
        assert!(events.iter().any(|e| matches!(e, Event::Resumable(..))));
    }

    #[test]
    fn each_refresh_gets_its_own_generation() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch_repo(&td));
        let (p, _rx) = loader();
        assert_eq!(p.refresh(&z, false), 1);
        assert_eq!(p.refresh(&z, false), 2);
        assert_eq!(p.generation(), 2);
    }

    #[test]
    fn results_carry_the_generation_they_were_started_for() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch_repo(&td));
        let (p, rx) = loader();
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
        let (p, rx) = loader();
        p.request_preview(&z, "w".into(), PreviewJob::Worktree(repo));

        let events = drain(p, &rx);
        let Some(Event::Preview(_, _, Preview::Worktree(lines))) = events.into_iter().next() else {
            panic!("no worktree preview");
        };
        let log = lines.join("\n");
        assert!(log.contains("zrush tests"), "no author in:\n{log}");
        assert!(log.contains("init"), "no subject in:\n{log}");
    }

    #[test]
    fn a_probe_on_a_directory_that_is_not_a_repo_fails_without_panicking() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let z = zrush(&td, repo.clone());
        // Make git unable to answer: the worktree list now points nowhere.
        std::fs::remove_dir_all(repo.join(".git")).unwrap();

        let (p, rx) = loader();
        p.refresh(&z, false);
        let events = drain(p, &rx);
        assert!(events.iter().any(|e| matches!(e, Event::Failed(..))));
    }

    #[test]
    fn a_loader_nobody_listens_to_stops_without_panicking() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        for b in ["a", "b", "c"] {
            git(
                &repo,
                &[
                    "worktree",
                    "add",
                    "-q",
                    &format!(".claude/worktrees/{b}"),
                    "-b",
                    b,
                ],
            );
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let sink: Sink = Arc::new(move |_| {
            seen.fetch_add(1, Ordering::SeqCst);
            false
        });
        let l = Loader::new(sink);
        assert_eq!(l.refresh(&zrush(&td, repo), false), 1);
        // The first answer is the worktree list; told nobody listens, the
        // round stops there instead of fanning out status probes.
        std::thread::sleep(Duration::from_secs(3));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
