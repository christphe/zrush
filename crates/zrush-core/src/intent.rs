//! What pressing enter on a row means, and the questions some rows raise
//! before anything happens.
//!
//! A question is data — a title, a body, the answers — so each interface
//! shows it its own way: a modal in the terminal, a dialog in a window. The
//! target is captured when the question is asked: rows are rebuilt while it
//! is up, and the answer must land on the session it was about.

use std::path::PathBuf;

use crate::model::tree::{NodeId, Row, RowKind};
use crate::snapshot::RepoSnapshot;

/// `enter` on a session whose transcript is filed under another
/// directory, which is the only case where opening it cannot resume it.
pub const MISFILED: &str = "Filed elsewhere";
pub const MISFILED_FILE: &str = "File it here and open";
pub const MISFILED_ANYWAY: &str = "Open anyway, fresh conversation";

/// Resuming an orphan. The agent files a conversation by the directory it
/// was started in and resumes only what it finds for the current one, so
/// putting the directory back is the answer that changes nothing else —
/// hence the default. Moving it is a real move, and says so.
pub const RESUME: &str = "Resume this session";
pub const RESUME_RECREATE: &str = "Recreate its worktree";
pub const RESUME_ROOT: &str = "Move it to the main worktree";

pub const CANCEL: &str = "Cancel";

/// What enter on a row comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    Nothing,
    /// Unfold more of this node's sessions.
    ShowMore(NodeId),
    /// A worktree always means a fresh conversation (`bind: None`); a
    /// session means resume that one.
    Open {
        worktree: PathBuf,
        bind: Option<(String, String)>,
    },
    Ask(Question),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub topic: Topic,
    pub title: String,
    pub body: String,
    /// In the order to offer them; the last is always `Cancel`.
    pub choices: Vec<Answer>,
}

/// What the question is about, captured when it was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Topic {
    Misfiled {
        agent: String,
        id: String,
        worktree: PathBuf,
    },
    Orphan {
        agent: String,
        id: String,
        /// Where it was held, when that is known.
        gone: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    FileHere,
    OpenAnyway,
    Recreate,
    MoveToMain,
    Cancel,
}

impl Answer {
    pub fn label(self) -> &'static str {
        match self {
            Self::FileHere => MISFILED_FILE,
            Self::OpenAnyway => MISFILED_ANYWAY,
            Self::Recreate => RESUME_RECREATE,
            Self::MoveToMain => RESUME_ROOT,
            Self::Cancel => CANCEL,
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        [
            Self::FileHere,
            Self::OpenAnyway,
            Self::Recreate,
            Self::MoveToMain,
            Self::Cancel,
        ]
        .into_iter()
        .find(|a| a.label() == label)
    }
}

/// What an answer comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    Open {
        worktree: PathBuf,
        bind: Option<(String, String)>,
    },
    /// File the conversation under `worktree`, then open it there.
    Adopt {
        agent: String,
        id: String,
        worktree: PathBuf,
    },
    /// A worktree must be created first, which needs a branch: the
    /// interface asks for one, then creates it at `at` handing `hand_over`
    /// (agent, id) to it.
    NeedBranch {
        at: Option<PathBuf>,
        hand_over: (String, String),
    },
}

pub fn enter(row: &Row, snap: &RepoSnapshot) -> Intent {
    match row.kind {
        RowKind::More => Intent::ShowMore(row.node.clone()),
        RowKind::Worktree => match &row.node {
            NodeId::Worktree(p) => Intent::Open {
                worktree: p.clone(),
                bind: None,
            },
            NodeId::Orphans => Intent::Nothing,
        },
        RowKind::Session => {
            let NodeId::Worktree(worktree) = &row.node else {
                return Intent::Nothing;
            };
            let Some((agent, id)) = bound(row, snap) else {
                return Intent::Nothing;
            };
            // Filed somewhere else, this very enter is what opens an empty
            // panel. Ask instead of doing it and saying nothing.
            if row.misfiled {
                return Intent::Ask(misfiled(row, agent, id, worktree.clone()));
            }
            Intent::Open {
                worktree: worktree.clone(),
                bind: Some((agent, id)),
            }
        }
        // An orphan's worktree is gone — removed, or never zrush's to begin
        // with. The conversation outlived it, so the question is where to
        // pick it up, and only you can answer that.
        RowKind::Orphan => {
            let Some((agent, id)) = bound(row, snap) else {
                return Intent::Nothing;
            };
            let gone = snap
                .session(&id)
                .map(|s| s.cwd.clone())
                .filter(|p| !p.as_os_str().is_empty());
            Intent::Ask(orphan(row, agent, id, gone))
        }
        RowKind::Orphans => Intent::Nothing,
    }
}

pub fn answer(q: &Question, a: Answer, snap: &RepoSnapshot) -> Outcome {
    match (&q.topic, a) {
        (
            Topic::Misfiled {
                agent,
                id,
                worktree,
            },
            Answer::FileHere,
        ) => Outcome::Adopt {
            agent: agent.clone(),
            id: id.clone(),
            worktree: worktree.clone(),
        },
        // What enter used to do, now chosen rather than suffered.
        (
            Topic::Misfiled {
                agent,
                id,
                worktree,
            },
            Answer::OpenAnyway,
        ) => Outcome::Open {
            worktree: worktree.clone(),
            bind: Some((agent.clone(), id.clone())),
        },
        (Topic::Orphan { agent, id, gone }, Answer::Recreate) => Outcome::NeedBranch {
            at: gone.clone(),
            hand_over: (agent.clone(), id.clone()),
        },
        (Topic::Orphan { agent, id, .. }, Answer::MoveToMain) => snap
            .worktrees
            .iter()
            .find(|w| w.is_main)
            .map_or(Outcome::Nothing, |w| Outcome::Adopt {
                agent: agent.clone(),
                id: id.clone(),
                worktree: w.path.clone(),
            }),
        _ => Outcome::Nothing,
    }
}

/// The session on this row and the agent it belongs to.
fn bound(row: &Row, snap: &RepoSnapshot) -> Option<(String, String)> {
    let id = row.session_id.clone()?;
    let agent = snap.session(&id)?.agent.to_string();
    Some((agent, id))
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn misfiled(row: &Row, agent: String, id: String, worktree: PathBuf) -> Question {
    let name = file_name(&worktree);
    Question {
        title: MISFILED.into(),
        body: format!(
            "{}\nIt was started elsewhere, so resuming it in {name} would\nopen an empty panel.",
            row.label.trim()
        ),
        choices: vec![Answer::FileHere, Answer::OpenAnyway, Answer::Cancel],
        topic: Topic::Misfiled {
            agent,
            id,
            worktree,
        },
    }
}

/// Where to resume an orphan: its old worktree back, or the main one.
/// Recreating is only offered when we know where it was: anywhere else
/// would not bring the conversation back.
fn orphan(row: &Row, agent: String, id: String, gone: Option<PathBuf>) -> Question {
    let where_ = gone.as_ref().map_or_else(
        || "Its worktree is gone.".to_string(),
        |p| format!("It was held in {}, which is gone.", file_name(p)),
    );
    let mut choices = Vec::new();
    if gone.is_some() {
        choices.push(Answer::Recreate);
    }
    choices.push(Answer::MoveToMain);
    choices.push(Answer::Cancel);
    Question {
        title: RESUME.into(),
        body: format!("{}\n{where_}", row.label.trim()),
        choices,
        topic: Topic::Orphan { agent, id, gone },
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::model::tree::{NodeId, Row, RowKind};
    use crate::model::{Session, SessionKind, Worktree};
    use crate::snapshot::RepoSnapshot;

    const MAIN: &str = "/r";
    const WT: &str = "/r/.claude/worktrees/wt";
    const GONE: &str = "/r/.claude/worktrees/gone-wt";

    fn worktree(path: &str, is_main: bool) -> Worktree {
        Worktree {
            path: path.into(),
            branch: "b".into(),
            locked: false,
            prunable: false,
            is_main,
        }
    }

    fn session(id: &str, cwd: &str) -> Session {
        Session {
            agent: "claude",
            id: id.into(),
            title: "t".into(),
            status: "resumable 2d".into(),
            cwd: cwd.into(),
            launch_cwd: cwd.into(),
            kind: SessionKind::Resumable,
            last_activity: 0,
        }
    }

    fn snap() -> RepoSnapshot {
        let mut s = RepoSnapshot::default();
        s.worktrees = vec![worktree(MAIN, true), worktree(WT, false)];
        s.resumable = vec![session("s1", WT), session("o1", GONE), session("o2", "")];
        s
    }

    fn row(kind: RowKind, node: NodeId, id: Option<&str>, misfiled: bool) -> Row {
        Row {
            kind,
            node,
            session_id: id.map(str::to_string),
            glyph: String::new(),
            label: "  my session  ".into(),
            search: String::new(),
            badge: String::new(),
            status: String::new(),
            misfiled,
            location: String::new(),
        }
    }

    fn wt_node() -> NodeId {
        NodeId::Worktree(WT.into())
    }

    #[test]
    fn enter_on_a_worktree_opens_it_unbound() {
        let r = row(RowKind::Worktree, wt_node(), None, false);
        assert!(matches!(
            enter(&r, &snap()),
            Intent::Open { worktree, bind: None } if worktree.as_path() == std::path::Path::new(WT)
        ));
    }

    #[test]
    fn enter_on_a_session_opens_it_bound() {
        let r = row(RowKind::Session, wt_node(), Some("s1"), false);
        assert!(matches!(
            enter(&r, &snap()),
            Intent::Open { bind: Some((a, i)), .. } if a == "claude" && i == "s1"
        ));
    }

    #[test]
    fn enter_on_a_more_row_shows_more() {
        let r = row(RowKind::More, wt_node(), None, false);
        assert!(matches!(enter(&r, &snap()), Intent::ShowMore(n) if n == wt_node()));
    }

    #[test]
    fn enter_on_a_session_nobody_knows_does_nothing() {
        let r = row(RowKind::Session, wt_node(), Some("ghost"), false);
        assert!(matches!(enter(&r, &snap()), Intent::Nothing));
    }

    #[test]
    fn enter_on_a_misfiled_session_asks_first() {
        let r = row(RowKind::Session, wt_node(), Some("s1"), true);
        let Intent::Ask(q) = enter(&r, &snap()) else {
            panic!("expected a question")
        };
        assert_eq!(q.title, MISFILED);
        assert_eq!(
            q.body,
            "my session\nIt was started elsewhere, so resuming it in wt would\nopen an empty panel."
        );
        assert_eq!(
            q.choices,
            vec![Answer::FileHere, Answer::OpenAnyway, Answer::Cancel]
        );
    }

    #[test]
    fn an_orphan_with_a_known_path_can_be_recreated() {
        let r = row(RowKind::Orphan, NodeId::Orphans, Some("o1"), false);
        let Intent::Ask(q) = enter(&r, &snap()) else {
            panic!("expected a question")
        };
        assert_eq!(q.title, RESUME);
        assert_eq!(q.body, "my session\nIt was held in gone-wt, which is gone.");
        assert_eq!(
            q.choices,
            vec![Answer::Recreate, Answer::MoveToMain, Answer::Cancel]
        );
    }

    #[test]
    fn an_orphan_with_no_known_path_cannot() {
        let r = row(RowKind::Orphan, NodeId::Orphans, Some("o2"), false);
        let Intent::Ask(q) = enter(&r, &snap()) else {
            panic!("expected a question")
        };
        assert_eq!(q.body, "my session\nIts worktree is gone.");
        assert_eq!(q.choices, vec![Answer::MoveToMain, Answer::Cancel]);
    }

    #[test]
    fn filing_a_misfiled_session_here_adopts_it_into_that_worktree() {
        let r = row(RowKind::Session, wt_node(), Some("s1"), true);
        let Intent::Ask(q) = enter(&r, &snap()) else {
            panic!()
        };
        assert!(matches!(
            answer(&q, Answer::FileHere, &snap()),
            Outcome::Adopt { worktree, id, .. } if worktree.as_path() == std::path::Path::new(WT) && id == "s1"
        ));
        assert!(matches!(
            answer(&q, Answer::OpenAnyway, &snap()),
            Outcome::Open { bind: Some(_), .. }
        ));
    }

    #[test]
    fn moving_an_orphan_to_main_adopts_it_there() {
        let r = row(RowKind::Orphan, NodeId::Orphans, Some("o1"), false);
        let Intent::Ask(q) = enter(&r, &snap()) else {
            panic!()
        };
        assert!(matches!(
            answer(&q, Answer::MoveToMain, &snap()),
            Outcome::Adopt { worktree, .. } if worktree.as_path() == std::path::Path::new(MAIN)
        ));
    }

    #[test]
    fn recreating_asks_for_a_branch_at_the_old_path() {
        let r = row(RowKind::Orphan, NodeId::Orphans, Some("o1"), false);
        let Intent::Ask(q) = enter(&r, &snap()) else {
            panic!()
        };
        assert!(matches!(
            answer(&q, Answer::Recreate, &snap()),
            Outcome::NeedBranch { at: Some(p), hand_over } if p.as_path() == std::path::Path::new(GONE) && hand_over.1 == "o1"
        ));
    }

    #[test]
    fn cancel_does_nothing() {
        let r = row(RowKind::Orphan, NodeId::Orphans, Some("o1"), false);
        let Intent::Ask(q) = enter(&r, &snap()) else {
            panic!()
        };
        assert!(matches!(
            answer(&q, Answer::Cancel, &snap()),
            Outcome::Nothing
        ));
    }

    #[test]
    fn labels_go_both_ways() {
        for a in [
            Answer::FileHere,
            Answer::OpenAnyway,
            Answer::Recreate,
            Answer::MoveToMain,
            Answer::Cancel,
        ] {
            assert_eq!(Answer::from_label(a.label()), Some(a));
        }
        assert_eq!(Answer::from_label("nope"), None);
    }
}
