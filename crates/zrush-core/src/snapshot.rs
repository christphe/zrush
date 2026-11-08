//! One repository as an interface sees it: what the loader has brought in
//! so far, and the rows it makes.
//!
//! The loader answers piece by piece — the worktree list first, then each
//! status, the live sessions, the scan. This holds the pieces, drops the
//! ones from a superseded round, and turns the lot into `tree::Row`s. An
//! interface keeps one per repository and only decides how to draw it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::agent::Filing;
use crate::app::Zrush;
use crate::error::Result;
use crate::loader::{Done, Event, Preview, PreviewJob};
use crate::model::tree::{self, NodeId, Row, RowKind, TreeInput};
use crate::model::{GitStatus, Session, SessionKind, Worktree};

#[derive(Debug, Default, Clone)]
pub struct RepoSnapshot {
    pub worktrees: Vec<Worktree>,
    pub statuses: HashMap<PathBuf, GitStatus>,
    /// The cheap per-worktree count, from one `readdir`, until the scan
    /// has answered.
    pub history: HashMap<PathBuf, usize>,
    pub live: Vec<Session>,
    pub resumable: Vec<Session>,
    /// The transcript scan has answered for the current worktree list.
    pub scanned: bool,
    /// Branches a new worktree could be opened on.
    pub branches: Vec<String>,
    previews: HashMap<String, Preview>,
    preview_pending: HashSet<String>,
}

/// How the interface wants the tree laid out: what is folded, how far each
/// node's sessions are unfolded, and the cap before that.
#[derive(Debug, Default, Clone)]
pub struct TreeView {
    pub collapsed: HashSet<NodeId>,
    pub expanded: HashMap<NodeId, usize>,
    pub resumable_max: usize,
    pub show_all: bool,
}

/// What an event changed, so the interface knows what to do next.
#[derive(Debug)]
pub enum Applied {
    /// From a superseded round: ignored.
    Stale,
    /// The rows are out of date: build them again.
    Rows,
    /// A preview arrived.
    Preview,
    /// A long action finished.
    Acted(Done),
    /// A probe gave up; worth a line, not an error box.
    Failed(String),
    /// A newer release, by its tag.
    Update(String),
    /// Stored; nothing to redraw.
    Nothing,
}

impl RepoSnapshot {
    /// Take one loader result. `generation` is the loader's current round:
    /// a result from an older one must not paint over the new one.
    pub fn apply(&mut self, event: Event, generation: u64) -> Applied {
        if event.generation().is_some_and(|g| g != generation) {
            return Applied::Stale;
        }
        match event {
            Event::Worktrees(_, w) => {
                self.worktrees = w;
                self.scanned = false;
                // Whatever was fetched described the previous state of the repo.
                self.previews.clear();
                self.preview_pending.clear();
                Applied::Rows
            }
            Event::Status(_, p, s) => {
                self.statuses.insert(p, s);
                Applied::Rows
            }
            Event::History(_, p, n) => {
                self.history.insert(p, n);
                Applied::Rows
            }
            Event::Live(_, s) => {
                self.live = s;
                Applied::Rows
            }
            Event::Branches(_, b) => {
                self.branches = b;
                Applied::Nothing
            }
            Event::Resumable(_, s) => {
                self.resumable = s;
                self.scanned = true;
                Applied::Rows
            }
            Event::Preview(_, key, preview) => {
                self.preview_pending.remove(&key);
                self.previews.insert(key, preview);
                Applied::Preview
            }
            Event::Acted(done) => Applied::Acted(done),
            Event::Failed(_, e) => Applied::Failed(e),
            Event::Update(tag) => Applied::Update(tag),
        }
    }

    /// The rows to draw, laid out as `view` says.
    pub fn rows(&self, z: &Zrush, view: &TreeView) -> Vec<Row> {
        let mut sessions = self.live.clone();
        sessions.extend(self.resumable.iter().cloned());
        let assigned = tree::assign(&sessions, &self.worktrees, z.main_root(), &z.new_root());

        // Until the scan has answered, the badge shows the cheap disk count.
        // Afterwards it counts what was actually assigned, so the two numbers
        // can never contradict each other.
        let history = if self.scanned {
            let mut counted: HashMap<PathBuf, usize> = HashMap::new();
            for a in &assigned {
                if let NodeId::Worktree(p) = &a.owner {
                    *counted.entry(p.clone()).or_default() += 1;
                }
            }
            counted
        } else {
            self.history.clone()
        };

        let mut rows = tree::build(&TreeInput {
            worktrees: &self.worktrees,
            assigned: &assigned,
            statuses: &self.statuses,
            history: &history,
            collapsed: &view.collapsed,
            expanded: &view.expanded,
            resumable_max: view.resumable_max,
            show_all: view.show_all,
        });
        mark_misfiled(&mut rows, &sessions, z);
        rows
    }

    /// Drop every session, keeping the worktrees: what was listed belonged
    /// to an agent that is no longer the one being looked at.
    pub fn forget_sessions(&mut self) {
        self.live.clear();
        self.resumable.clear();
        self.scanned = false;
    }

    /// A session by id: running ones first, then the ones that can resume.
    pub fn session(&self, id: &str) -> Option<&Session> {
        self.live
            .iter()
            .chain(self.resumable.iter())
            .find(|s| s.id == id)
    }

    /// Every session held in `worktree` or below it.
    pub fn sessions_in(&self, worktree: &Path) -> Vec<Session> {
        self.live
            .iter()
            .chain(self.resumable.iter())
            .filter(|s| s.cwd.starts_with(worktree))
            .cloned()
            .collect()
    }

    /// What has been fetched for this preview key, or `Empty`.
    pub fn preview(&self, key: &str) -> Preview {
        self.previews.get(key).cloned().unwrap_or(Preview::Empty)
    }

    /// The fetch this row's preview needs, once: `None` when it is already
    /// here or on its way, or when the row has no preview. `listed_agent`
    /// is the fallback for a session this snapshot does not know.
    pub fn want_preview(&mut self, row: &Row, listed_agent: &str) -> Option<(String, PreviewJob)> {
        let key = preview_key(row)?;
        if self.previews.contains_key(&key) || self.preview_pending.contains(&key) {
            return None;
        }
        let job = match row.kind {
            RowKind::Session | RowKind::Orphan => {
                let id = row.session_id.clone()?;
                let agent = self
                    .session(&id)
                    .map_or(listed_agent, |s| s.agent)
                    .to_string();
                PreviewJob::Conversation { agent, id }
            }
            RowKind::Worktree => match &row.node {
                NodeId::Worktree(p) => PreviewJob::Worktree(p.clone()),
                NodeId::Orphans => return None,
            },
            RowKind::Orphans | RowKind::More => return None,
        };
        self.preview_pending.insert(key.clone());
        Some((key, job))
    }

    /// Everything, now, on this thread — for a one-shot listing rather
    /// than an interface that wants the first answer early. Not marked
    /// scanned: the badge keeps the count of transcripts on disk, which is
    /// what `--list` has always printed.
    pub fn load(z: &Zrush) -> Result<Self> {
        let worktrees = z.worktrees()?;
        let live = z.live_sessions();
        let resumable = z.resumable_sessions(&worktrees, &live, false);
        let history = worktrees
            .iter()
            .map(|w| (w.path.clone(), z.history_count(&w.path)))
            .collect();
        let statuses = worktrees
            .iter()
            .filter_map(|w| z.status(&w.path).ok().map(|s| (w.path.clone(), s)))
            .collect();
        Ok(Self {
            worktrees,
            statuses,
            history,
            live,
            resumable,
            ..Self::default()
        })
    }
}

/// Identifies a row for the preview cache: a session by its id, a
/// worktree by its path. Stable across a reload.
pub fn preview_key(row: &Row) -> Option<String> {
    match row.kind {
        RowKind::Session | RowKind::Orphan => row.session_id.as_ref().map(|id| format!("s:{id}")),
        RowKind::Worktree => match &row.node {
            NodeId::Worktree(p) => Some(format!("w:{}", p.display())),
            NodeId::Orphans => None,
        },
        RowKind::Orphans | RowKind::More => None,
    }
}

/// Say so when a session is listed somewhere the agent would not find it.
///
/// zrush assigns a conversation to the worktree it last worked in; the
/// agent files it under the one it was *started* in, and never moves it.
/// Resuming from the row then opens an empty panel and says nothing, which
/// is the most confusing thing zrush can do.
fn mark_misfiled(rows: &mut [Row], sessions: &[Session], z: &Zrush) {
    for row in rows {
        let (RowKind::Session | RowKind::Orphan) = row.kind else {
            continue;
        };
        let NodeId::Worktree(worktree) = &row.node else {
            continue;
        };
        let Some(id) = row.session_id.clone() else {
            continue;
        };
        // A live session is already attached to its panel; where its
        // transcript sits only matters to resuming one.
        let Some(session) = sessions
            .iter()
            .find(|s| s.id == id && s.kind == SessionKind::Resumable)
        else {
            continue;
        };
        if z.filing(session.agent, &id, worktree) == Filing::Elsewhere {
            row.status = format!("{} · filed elsewhere", row.status);
            row.misfiled = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::loader::{Event, Preview, PreviewJob};
    use crate::model::tree::{NodeId, Row, RowKind};
    use crate::model::{Session, SessionKind, Worktree};
    use crate::testing::{scratch_repo, zrush};

    fn wt(path: &str, is_main: bool) -> Worktree {
        Worktree {
            path: path.into(),
            branch: Path::new(path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            locked: false,
            prunable: false,
            is_main,
        }
    }

    fn session(id: &str, cwd: &str, kind: SessionKind) -> Session {
        Session {
            agent: "claude",
            id: id.into(),
            title: format!("title {id}"),
            status: "resumable 2d".into(),
            cwd: cwd.into(),
            launch_cwd: cwd.into(),
            kind,
            last_activity: 0,
        }
    }

    fn view() -> TreeView {
        TreeView {
            resumable_max: 5,
            ..TreeView::default()
        }
    }

    #[test]
    fn a_result_from_an_old_round_is_stale() {
        let mut s = RepoSnapshot::default();
        assert!(matches!(s.apply(Event::Live(1, vec![]), 2), Applied::Stale));
    }

    #[test]
    fn a_new_worktree_list_forgets_the_previews_and_the_scan() {
        let mut s = RepoSnapshot::default();
        s.apply(
            Event::Preview(1, "w:/a".into(), Preview::Worktree(vec!["x".into()])),
            1,
        );
        s.apply(Event::Resumable(1, vec![]), 1);
        assert!(s.scanned);
        assert!(matches!(
            s.apply(Event::Worktrees(1, vec![wt("/a", true)]), 1),
            Applied::Rows
        ));
        assert_eq!(s.preview("w:/a"), Preview::Empty);
        assert!(!s.scanned);
    }

    #[test]
    fn the_badge_counts_what_was_assigned_once_scanned() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let z = zrush(&td, repo.clone());
        let main = repo.to_str().unwrap().to_string();
        let mut s = RepoSnapshot::default();
        s.apply(Event::Worktrees(1, z.worktrees().unwrap()), 1);
        s.apply(Event::History(1, repo.clone(), 7), 1);
        let badge = |s: &RepoSnapshot| s.rows(&z, &view())[0].badge.clone();
        let before = badge(&s);
        s.apply(
            Event::Resumable(
                1,
                vec![
                    session("a", &main, SessionKind::Resumable),
                    session("b", &main, SessionKind::Resumable),
                ],
            ),
            1,
        );
        let after = badge(&s);
        assert!(before.contains('7'), "before the scan: {before}");
        assert!(
            after.contains('2') && !after.contains('7'),
            "after: {after}"
        );
    }

    #[test]
    fn a_worktree_row_carries_its_facts_not_only_its_text() {
        // An interface that is not a terminal draws these its own way, so
        // they come as data rather than inside the label.
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let z = zrush(&td, repo.clone());
        let main = repo.to_str().unwrap().to_string();
        let mut s = RepoSnapshot::default();
        s.apply(Event::Worktrees(1, z.worktrees().unwrap()), 1);
        s.apply(Event::History(1, repo.clone(), 4), 1);
        s.apply(
            Event::Live(1, vec![session("l", &main, SessionKind::Live)]),
            1,
        );
        let row = &s.rows(&z, &view())[0];
        assert!(row.facts.root);
        assert_eq!(row.facts.live, 1);
        assert_eq!(row.facts.history, 4);
    }

    #[test]
    fn a_preview_is_asked_for_once() {
        let mut s = RepoSnapshot::default();
        let row = Row {
            facts: crate::model::tree::RowFacts::default(),
            kind: RowKind::Worktree,
            node: NodeId::Worktree("/a".into()),
            session_id: None,
            glyph: String::new(),
            label: "a".into(),
            search: "a".into(),
            badge: String::new(),
            status: String::new(),
            misfiled: false,
            location: "/a".into(),
        };
        let first = s.want_preview(&row, "claude");
        assert!(matches!(first, Some((_, PreviewJob::Worktree(_)))));
        assert!(s.want_preview(&row, "claude").is_none());
    }

    #[test]
    fn forgetting_sessions_keeps_the_worktrees() {
        let mut s = RepoSnapshot::default();
        s.apply(Event::Worktrees(1, vec![wt("/a", true)]), 1);
        s.apply(
            Event::Live(1, vec![session("l", "/a", SessionKind::Live)]),
            1,
        );
        s.apply(
            Event::Resumable(1, vec![session("r", "/a", SessionKind::Resumable)]),
            1,
        );
        s.forget_sessions();
        assert!(s.live.is_empty() && s.resumable.is_empty() && !s.scanned);
        assert_eq!(s.worktrees.len(), 1);
    }

    #[test]
    fn a_session_is_found_live_or_resumable() {
        let s = RepoSnapshot {
            live: vec![session("l", "/a", SessionKind::Live)],
            resumable: vec![session("r", "/a", SessionKind::Resumable)],
            ..RepoSnapshot::default()
        };
        assert_eq!(s.session("l").unwrap().kind, SessionKind::Live);
        assert_eq!(s.session("r").unwrap().kind, SessionKind::Resumable);
        assert!(s.session("x").is_none());
    }

    #[test]
    fn sessions_in_a_worktree_include_its_subdirectories() {
        let s = RepoSnapshot {
            resumable: vec![
                session("in", "/r/wt/sub", SessionKind::Resumable),
                session("out", "/r/wt2", SessionKind::Resumable),
            ],
            ..RepoSnapshot::default()
        };
        let ids: Vec<String> = s
            .sessions_in(Path::new("/r/wt"))
            .into_iter()
            .map(|x| x.id)
            .collect();
        assert_eq!(ids, vec!["in".to_string()]);
    }

    #[test]
    fn a_live_session_filed_elsewhere_is_never_marked() {
        // A running session is already attached to its panel: where its
        // transcript sits only matters to resuming one.
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let z = zrush(&td, repo.clone());
        let elsewhere = z
            .dirs()
            .claude_projects
            .join(crate::agent::claude::projects::slugify(Path::new(
                "/somewhere/else",
            )));
        std::fs::create_dir_all(&elsewhere).unwrap();
        let live_id = "aaaaaaaa-1111-2222-3333-444455556666";
        let dead_id = "bbbbbbbb-1111-2222-3333-444455556666";
        for id in [live_id, dead_id] {
            std::fs::write(elsewhere.join(format!("{id}.jsonl")), "").unwrap();
        }
        let here = repo.to_str().unwrap();
        let mut s = RepoSnapshot::default();
        s.apply(Event::Worktrees(1, z.worktrees().unwrap()), 1);
        s.apply(
            Event::Live(1, vec![session(live_id, here, SessionKind::Live)]),
            1,
        );
        s.apply(
            Event::Resumable(1, vec![session(dead_id, here, SessionKind::Resumable)]),
            1,
        );
        let rows = s.rows(&z, &view());
        let row = |id: &str| {
            rows.iter()
                .find(|r| r.session_id.as_deref() == Some(id))
                .unwrap()
                .clone()
        };
        assert!(row(dead_id).misfiled, "the setup must file them elsewhere");
        let live = row(live_id);
        assert!(!live.misfiled);
        assert!(matches!(
            crate::intent::enter(&live, &s),
            crate::intent::Intent::Open { bind: Some(_), .. }
        ));
    }

    #[test]
    fn loading_at_once_gets_everything() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch_repo(&td));
        let s = RepoSnapshot::load(&z).unwrap();
        assert_eq!(s.worktrees.len(), 1);
        assert_eq!(s.rows(&z, &view()).len(), 1);
    }

    #[test]
    fn loading_at_once_keeps_the_disk_count_in_the_badge() {
        // `--list` has always shown how many transcripts sit on disk, not
        // how many the capped scan assigned; scripts read that column.
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let z = zrush(&td, repo.clone());
        let dir = z
            .dirs()
            .claude_projects
            .join(crate::agent::claude::projects::slugify(&repo));
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..3 {
            std::fs::write(dir.join(format!("{i}.jsonl")), "").unwrap();
        }
        let s = RepoSnapshot::load(&z).unwrap();
        let badge = s.rows(&z, &view())[0].badge.clone();
        assert!(badge.contains('3'), "badge: {badge}");
    }
}
