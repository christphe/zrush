//! Purge: the marked rows of a tree turned into what goes, by the same rules
//! whatever interface marked them. The main worktree never goes, a running
//! session never goes, and a session in a worktree that git refuses to
//! remove stays with it.

use std::fmt::Write as _;
use std::path::PathBuf;

use crate::model::SessionKind;
use crate::model::tree::{NodeId, Row, RowKind};
use crate::snapshot::RepoSnapshot;

/// A conversation to delete, and the worktree it is listed under: `None`
/// for an orphan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Doomed {
    pub agent: String,
    pub id: String,
    pub owner: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub worktrees: Vec<PathBuf>,
    pub sessions: Vec<Doomed>,
    /// Marked sessions left alone because they are running.
    pub kept_live: usize,
    /// The main worktree was marked, and is left alone.
    pub skipped_main: bool,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.worktrees.is_empty() && self.sessions.is_empty()
    }

    /// The other conversations held in the marked worktrees join the plan,
    /// running ones aside: "and their other sessions too".
    #[must_use]
    pub fn with_others(mut self, snap: &RepoSnapshot) -> Self {
        for w in self.worktrees.clone() {
            for s in snap.sessions_in(&w) {
                if s.kind == SessionKind::Live || self.sessions.iter().any(|d| d.id == s.id) {
                    continue;
                }
                self.sessions.push(Doomed {
                    agent: s.agent.to_string(),
                    id: s.id,
                    owner: Some(w.clone()),
                });
            }
        }
        self
    }

    /// What a confirmation says before anything goes.
    pub fn summary(&self) -> String {
        let mut s = format!(
            "{} worktree(s) and {} session(s).",
            self.worktrees.len(),
            self.sessions.len()
        );
        if self.kept_live > 0 {
            let _ = write!(s, "\n{} running session(s) kept.", self.kept_live);
        }
        if self.skipped_main {
            s.push_str("\nThe main worktree is skipped.");
        }
        s
    }
}

/// The plan for the rows at `marked`, in row order.
pub fn plan(rows: &[Row], marked: impl IntoIterator<Item = usize>, snap: &RepoSnapshot) -> Plan {
    let mut marked: Vec<usize> = marked.into_iter().collect();
    marked.sort_unstable();
    marked.dedup();
    let main = snap.worktrees.iter().find(|w| w.is_main).map(|w| &w.path);
    let mut out = Plan::default();
    for row in marked.into_iter().filter_map(|i| rows.get(i)) {
        match (row.kind, &row.node) {
            (RowKind::Worktree, NodeId::Worktree(p)) => {
                if Some(p) == main {
                    out.skipped_main = true;
                } else {
                    out.worktrees.push(p.clone());
                }
            }
            (RowKind::Session | RowKind::Orphan, node) => {
                let Some(s) = row.session_id.as_deref().and_then(|id| snap.session(id)) else {
                    continue;
                };
                if s.kind == SessionKind::Live {
                    out.kept_live += 1;
                    continue;
                }
                out.sessions.push(Doomed {
                    agent: s.agent.to_string(),
                    id: s.id.clone(),
                    owner: match node {
                        NodeId::Worktree(p) => Some(p.clone()),
                        NodeId::Orphans => None,
                    },
                });
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tree::RowFacts;
    use crate::model::{Session, Worktree};

    const MAIN: &str = "/repo";
    const X: &str = "/repo/.claude/worktrees/x";

    fn row(kind: RowKind, node: NodeId, id: Option<&str>) -> Row {
        Row {
            facts: RowFacts::default(),
            kind,
            node,
            session_id: id.map(str::to_string),
            glyph: String::new(),
            label: String::new(),
            search: String::new(),
            badge: String::new(),
            status: String::new(),
            misfiled: false,
            location: String::new(),
        }
    }

    fn session(id: &str, cwd: &str, kind: SessionKind) -> Session {
        Session {
            agent: "claude",
            id: id.into(),
            title: String::new(),
            status: String::new(),
            cwd: cwd.into(),
            launch_cwd: cwd.into(),
            kind,
            last_activity: 0,
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

    /// main; x with a dead session, a running one and another dead one;
    /// one orphan.
    fn tree() -> (Vec<Row>, RepoSnapshot) {
        let x = NodeId::Worktree(X.into());
        let mut snap = RepoSnapshot::default();
        snap.worktrees = vec![wt(MAIN, true), wt(X, false)];
        snap.live = vec![session("alive", X, SessionKind::Live)];
        snap.resumable = vec![
            session("dead", X, SessionKind::Resumable),
            session("other", X, SessionKind::Resumable),
            session("lost", "/gone", SessionKind::Resumable),
        ];
        let rows = vec![
            row(RowKind::Worktree, NodeId::Worktree(MAIN.into()), None),
            row(RowKind::Worktree, x.clone(), None),
            row(RowKind::Session, x.clone(), Some("dead")),
            row(RowKind::Session, x.clone(), Some("alive")),
            row(RowKind::Session, x, Some("other")),
            row(RowKind::Orphans, NodeId::Orphans, None),
            row(RowKind::Orphan, NodeId::Orphans, Some("lost")),
        ];
        (rows, snap)
    }

    fn ids(p: &Plan) -> Vec<&str> {
        p.sessions.iter().map(|d| d.id.as_str()).collect()
    }

    #[test]
    fn the_main_worktree_is_skipped() {
        let (rows, snap) = tree();
        let p = plan(&rows, [0], &snap);
        assert_eq!(p.worktrees, Vec::<PathBuf>::new());
        assert!(p.skipped_main);
        assert!(p.is_empty());
    }

    #[test]
    fn a_running_session_is_never_purged() {
        let (rows, snap) = tree();
        let p = plan(&rows, [3], &snap);
        assert_eq!(p.sessions, Vec::new());
        assert_eq!(p.kept_live, 1);
    }

    #[test]
    fn a_marked_session_under_a_marked_worktree_is_still_deleted() {
        // Removing a worktree leaves its transcripts on disk: a session
        // marked with it has to be deleted on its own account.
        let (rows, snap) = tree();
        let p = plan(&rows, [1, 2], &snap);
        assert_eq!(p.worktrees, vec![PathBuf::from(X)]);
        assert_eq!(ids(&p), vec!["dead"]);
        assert_eq!(
            p.sessions[0].owner.as_deref(),
            Some(std::path::Path::new(X))
        );
    }

    #[test]
    fn an_orphan_has_no_owner() {
        let (rows, snap) = tree();
        let p = plan(&rows, [6], &snap);
        assert_eq!(ids(&p), vec!["lost"]);
        assert_eq!(p.sessions[0].owner, None);
    }

    #[test]
    fn the_other_sessions_of_a_marked_worktree_join_except_running_ones() {
        let (rows, snap) = tree();
        let p = plan(&rows, [1, 2], &snap).with_others(&snap);
        assert_eq!(ids(&p), vec!["dead", "other"]);
    }

    #[test]
    fn the_summary_counts_and_says_what_is_left_alone() {
        let (rows, snap) = tree();
        let p = plan(&rows, [0, 1, 3], &snap);
        assert_eq!(
            p.summary(),
            "1 worktree(s) and 0 session(s).\n1 running session(s) kept.\nThe main worktree is skipped."
        );
    }
}
