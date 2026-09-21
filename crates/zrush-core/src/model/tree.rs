//! Turning worktrees and sessions into the rows the table draws.
//!
//! Nothing in here knows about ratatui or the terminal: it is the port of
//! the awk the bash version used, as ordinary functions over ordinary data.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::model::{GitStatus, Session, SessionKind, Worktree};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NodeId {
    Worktree(PathBuf),
    Orphans,
}

#[derive(Debug, Clone)]
pub struct Assigned {
    pub owner: NodeId,
    pub session: Session,
    /// For an orphan: the worktree the session was written for.
    pub origin: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Worktree,
    Session,
    Orphans,
    Orphan,
    More,
}

#[derive(Debug, Clone)]
pub struct Row {
    pub kind: RowKind,
    pub node: NodeId,
    pub session_id: Option<String>,
    /// `▾ `, `▸ ` or two spaces for a node; the tree glyph for a child.
    pub glyph: String,
    pub label: String,
    pub badge: String,
    pub status: String,
    pub location: String,
}

impl Row {
    pub fn is_child(&self) -> bool {
        matches!(
            self.kind,
            RowKind::Session | RowKind::Orphan | RowKind::More
        )
    }
}

/// `parent` contains `child` only on a component boundary: `/a/b` contains
/// `/a/b/c` but not `/a/bc`.
fn contains(parent: &Path, child: &Path) -> bool {
    child == parent || child.starts_with(parent)
}

fn owner_of(cwd: &Path, worktrees: &[Worktree]) -> Option<PathBuf> {
    worktrees
        .iter()
        .filter(|w| contains(&w.path, cwd))
        .max_by_key(|w| w.path.components().count())
        .map(|w| w.path.clone())
}

/// The worktree a path was *meant* for, when that worktree is gone. A deleted
/// `<main>/.claude/worktrees/<name>` still has `<main>` as a prefix, so
/// `owner_of` would quietly file the session under the main worktree with the
/// other hundred. Name the dead one instead.
fn dead_worktree(cwd: &Path, new_root: &Path, worktrees: &[Worktree]) -> Option<String> {
    let rest = cwd.strip_prefix(new_root).ok()?;
    let seg = rest
        .components()
        .next()?
        .as_os_str()
        .to_string_lossy()
        .into_owned();
    if seg.is_empty() {
        return None;
    }
    let candidate = new_root.join(&seg);
    worktrees.iter().all(|w| w.path != candidate).then_some(seg)
}

/// Attach each session to the worktree that is the longest prefix of its
/// effective cwd, falling back to its launch cwd.
pub fn assign(
    sessions: &[Session],
    worktrees: &[Worktree],
    main_root: &Path,
    new_root: &Path,
) -> Vec<Assigned> {
    let mut out = Vec::new();
    for s in sessions {
        if let Some(from) = dead_worktree(&s.cwd, new_root, worktrees) {
            out.push(Assigned {
                owner: NodeId::Orphans,
                session: s.clone(),
                origin: Some(from),
            });
            continue;
        }
        if let Some(p) = owner_of(&s.cwd, worktrees).or_else(|| owner_of(&s.launch_cwd, worktrees))
        {
            out.push(Assigned {
                owner: NodeId::Worktree(p),
                session: s.clone(),
                origin: None,
            });
            continue;
        }
        // Only paths inside this repo become orphans; a session from another
        // repository is simply not ours to show.
        if contains(main_root, &s.cwd) {
            let base = s.cwd.file_name().map(|b| b.to_string_lossy().into_owned());
            out.push(Assigned {
                owner: NodeId::Orphans,
                session: s.clone(),
                origin: base,
            });
        }
    }
    out
}

pub struct TreeInput<'a> {
    pub worktrees: &'a [Worktree],
    pub assigned: &'a [Assigned],
    pub statuses: &'a HashMap<PathBuf, GitStatus>,
    pub history: &'a HashMap<PathBuf, usize>,
    pub collapsed: &'a HashSet<NodeId>,
    pub expanded: &'a HashMap<NodeId, usize>,
    pub resumable_max: usize,
    pub show_all: bool,
}

pub fn build(input: &TreeInput<'_>) -> Vec<Row> {
    let mut by_node: HashMap<&NodeId, Vec<&Assigned>> = HashMap::new();
    for a in input.assigned {
        by_node.entry(&a.owner).or_default().push(a);
    }
    // Newest first, live and resumable interleaved: the cap then keeps the
    // most recent ones, and the rows come out in date order for free.
    for v in by_node.values_mut() {
        v.sort_by_key(|a| std::cmp::Reverse(a.session.last_activity));
    }

    let mut rows = Vec::new();
    for w in input.worktrees {
        let node = NodeId::Worktree(w.path.clone());
        let empty: Vec<&Assigned> = Vec::new();
        let children = by_node.get(&node).unwrap_or(&empty);
        let (shown, hidden) = split_at_cap(children, cap_for(input, &node));
        let collapsed = input.collapsed.contains(&node);
        rows.push(Row {
            kind: RowKind::Worktree,
            node: node.clone(),
            session_id: None,
            glyph: fold_glyph(!children.is_empty(), collapsed),
            label: w.label(),
            badge: session_badge(children, input.history.get(&w.path).copied().unwrap_or(0)),
            status: input
                .statuses
                .get(&w.path)
                .map(GitStatus::badge)
                .unwrap_or_default(),
            location: w.path.to_string_lossy().into_owned(),
        });
        if !collapsed {
            push_children(&mut rows, &node, &shown, hidden, RowKind::Session);
        }
    }

    // Sessions whose worktree is gone. They would otherwise land under the
    // main worktree by prefix and vanish among its hundred, so they get their
    // own node and each row says which worktree it was written for.
    let node = NodeId::Orphans;
    if let Some(children) = by_node.get(&node) {
        let (shown, hidden) = split_at_cap(children, cap_for(input, &node));
        let collapsed = input.collapsed.contains(&node);
        rows.push(Row {
            kind: RowKind::Orphans,
            node: node.clone(),
            session_id: None,
            glyph: fold_glyph(true, collapsed),
            label: "orphaned sessions".into(),
            badge: format!("◌ {}", children.len()),
            status: String::new(),
            location: "worktree gone".into(),
        });
        if !collapsed {
            push_children(&mut rows, &node, &shown, hidden, RowKind::Orphan);
        }
    }
    rows
}

fn fold_glyph(has_children: bool, collapsed: bool) -> String {
    if !has_children {
        "  ".into()
    } else if collapsed {
        "▸ ".into()
    } else {
        "▾ ".into()
    }
}

/// Live sessions are never capped; only resumables past the limit are held
/// back, and they become one `[…more]` row.
fn split_at_cap<'a>(children: &[&'a Assigned], cap: usize) -> (Vec<&'a Assigned>, usize) {
    let mut shown = Vec::new();
    let mut resum = 0usize;
    let mut hidden = 0usize;
    for a in children {
        if a.session.kind == SessionKind::Live {
            shown.push(*a);
            continue;
        }
        resum += 1;
        if resum <= cap {
            shown.push(*a);
        } else {
            hidden += 1;
        }
    }
    (shown, hidden)
}

fn cap_for(input: &TreeInput<'_>, node: &NodeId) -> usize {
    if input.show_all {
        return usize::MAX;
    }
    input
        .expanded
        .get(node)
        .copied()
        .unwrap_or(input.resumable_max)
}

fn session_badge(children: &[&Assigned], history: usize) -> String {
    let live = children
        .iter()
        .filter(|a| a.session.kind == SessionKind::Live)
        .count();
    if live > 0 {
        format!("● {live} live")
    } else if history > 0 {
        format!("◌ {history} resumable")
    } else {
        String::new()
    }
}

fn push_children(
    rows: &mut Vec<Row>,
    node: &NodeId,
    shown: &[&Assigned],
    hidden: usize,
    kind: RowKind,
) {
    let last = shown.len();
    for (i, a) in shown.iter().enumerate() {
        let closing = i + 1 == last && hidden == 0;
        let tree = if closing { "└─" } else { "├─" };
        let dot = if a.session.kind == SessionKind::Live {
            "●"
        } else {
            "◌"
        };
        let mut status = a.session.status.clone();
        if let Some(origin) = &a.origin {
            status = format!("{status}  {origin}");
        }
        rows.push(Row {
            kind,
            node: node.clone(),
            session_id: Some(a.session.id.clone()),
            glyph: String::new(),
            label: format!("   {tree} {dot} {}", a.session.title),
            badge: status,
            status: String::new(),
            location: String::new(),
        });
    }
    if hidden > 0 {
        rows.push(Row {
            kind: RowKind::More,
            node: node.clone(),
            session_id: None,
            glyph: String::new(),
            label: "   └─ […more]".into(),
            badge: format!("+{hidden} older"),
            status: String::new(),
            location: String::new(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN: &str = "/home/u/repo";
    const NEW: &str = "/home/u/repo/.claude/worktrees";

    fn wt(path: &str, is_main: bool) -> Worktree {
        Worktree {
            path: path.into(),
            branch: "b".into(),
            locked: false,
            prunable: false,
            is_main,
        }
    }

    fn sess(cwd: &str, launch: &str, kind: SessionKind) -> Session {
        Session {
            agent: "claude",
            id: "aaaaaaaa-1111-2222-3333-444455556666".into(),
            title: "t".into(),
            status: "running".into(),
            cwd: cwd.into(),
            launch_cwd: launch.into(),
            kind,
            last_activity: 0,
        }
    }

    fn fixture() -> Vec<Worktree> {
        vec![
            wt(MAIN, true),
            wt("/home/u/repo/.claude/worktrees/rust", false),
        ]
    }

    fn owned(path: &str, kind: SessionKind) -> Assigned {
        Assigned {
            owner: NodeId::Worktree(path.into()),
            session: sess(path, path, kind),
            origin: None,
        }
    }

    fn input<'a>(
        worktrees: &'a [Worktree],
        assigned: &'a [Assigned],
        collapsed: &'a HashSet<NodeId>,
        expanded: &'a HashMap<NodeId, usize>,
        statuses: &'a HashMap<PathBuf, GitStatus>,
        history: &'a HashMap<PathBuf, usize>,
    ) -> TreeInput<'a> {
        TreeInput {
            worktrees,
            assigned,
            statuses,
            history,
            collapsed,
            expanded,
            resumable_max: 5,
            show_all: false,
        }
    }

    // ---------------------------------------------------------- assign ---

    #[test]
    fn the_longest_prefix_wins() {
        // Both /home/u/repo and .../worktrees/rust are prefixes. The deeper
        // one must win, or every session lands on the main worktree.
        let a = assign(
            &[sess(
                "/home/u/repo/.claude/worktrees/rust/src",
                "/x",
                SessionKind::Live,
            )],
            &fixture(),
            MAIN.as_ref(),
            NEW.as_ref(),
        );
        assert_eq!(
            a[0].owner,
            NodeId::Worktree("/home/u/repo/.claude/worktrees/rust".into())
        );
    }

    #[test]
    fn a_partial_path_component_is_not_a_prefix() {
        let a = assign(
            &[sess("/home/u/repo-other", "/x", SessionKind::Live)],
            &fixture(),
            MAIN.as_ref(),
            NEW.as_ref(),
        );
        assert!(a.is_empty());
    }

    #[test]
    fn the_launch_cwd_is_the_fallback() {
        let a = assign(
            &[sess("/elsewhere", MAIN, SessionKind::Live)],
            &fixture(),
            MAIN.as_ref(),
            NEW.as_ref(),
        );
        assert_eq!(a[0].owner, NodeId::Worktree(MAIN.into()));
    }

    #[test]
    fn a_session_of_a_deleted_worktree_becomes_an_orphan_named_after_it() {
        let a = assign(
            &[sess(
                "/home/u/repo/.claude/worktrees/gone/src",
                "/x",
                SessionKind::Live,
            )],
            &fixture(),
            MAIN.as_ref(),
            NEW.as_ref(),
        );
        assert_eq!(a[0].owner, NodeId::Orphans);
        assert_eq!(a[0].origin.as_deref(), Some("gone"));
    }

    #[test]
    fn a_stray_path_under_the_main_worktree_is_an_orphan_too() {
        let a = assign(
            &[sess("/home/u/repo", "/nowhere", SessionKind::Live)],
            &[wt("/other", true)],
            MAIN.as_ref(),
            NEW.as_ref(),
        );
        assert_eq!(a[0].owner, NodeId::Orphans);
        assert_eq!(a[0].origin.as_deref(), Some("repo"));
    }

    #[test]
    fn a_session_from_another_repository_is_not_ours_to_show() {
        let a = assign(
            &[sess("/somewhere/else", "/also/else", SessionKind::Live)],
            &fixture(),
            MAIN.as_ref(),
            NEW.as_ref(),
        );
        assert!(a.is_empty());
    }

    #[test]
    fn a_live_worktree_under_new_root_is_not_treated_as_dead() {
        let a = assign(
            &[sess(
                "/home/u/repo/.claude/worktrees/rust",
                "/x",
                SessionKind::Live,
            )],
            &fixture(),
            MAIN.as_ref(),
            NEW.as_ref(),
        );
        assert_eq!(
            a[0].owner,
            NodeId::Worktree("/home/u/repo/.claude/worktrees/rust".into())
        );
        assert_eq!(a[0].origin, None);
    }

    // ----------------------------------------------------------- build ---

    #[test]
    fn a_worktree_with_no_sessions_has_no_fold_marker() {
        let w = vec![wt(MAIN, true)];
        let rows = build(&input(
            &w,
            &[],
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        ));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].glyph, "  ");
    }

    #[test]
    fn folding_hides_the_session_rows_but_keeps_the_worktree() {
        let w = vec![wt(MAIN, true)];
        let a = vec![owned(MAIN, SessionKind::Live)];
        let open = build(&input(
            &w,
            &a,
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        ));
        assert_eq!(open.len(), 2);
        assert_eq!(open[0].glyph, "▾ ");

        let mut collapsed = HashSet::new();
        collapsed.insert(NodeId::Worktree(MAIN.into()));
        let shut = build(&input(
            &w,
            &a,
            &collapsed,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        ));
        assert_eq!(shut.len(), 1);
        assert_eq!(shut[0].glyph, "▸ ");
    }

    #[test]
    fn the_last_session_gets_the_closing_glyph() {
        let w = vec![wt(MAIN, true)];
        let a = vec![
            owned(MAIN, SessionKind::Live),
            owned(MAIN, SessionKind::Live),
        ];
        let rows = build(&input(
            &w,
            &a,
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        ));
        assert!(rows[1].label.contains("├─"));
        assert!(rows[2].label.contains("└─"));
    }

    #[test]
    fn resumables_past_the_cap_become_a_more_row() {
        let w = vec![wt(MAIN, true)];
        let a: Vec<Assigned> = (0..8)
            .map(|_| owned(MAIN, SessionKind::Resumable))
            .collect();
        let rows = build(&input(
            &w,
            &a,
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        ));
        assert_eq!(rows.len(), 7); // worktree + 5 shown + the more row
        assert_eq!(rows[6].kind, RowKind::More);
        assert_eq!(rows[6].badge, "+3 older");
    }

    #[test]
    fn expanding_raises_that_worktrees_cap_only() {
        let w = vec![wt(MAIN, true)];
        let a: Vec<Assigned> = (0..8)
            .map(|_| owned(MAIN, SessionKind::Resumable))
            .collect();
        let mut expanded = HashMap::new();
        expanded.insert(NodeId::Worktree(MAIN.into()), 25);
        let rows = build(&input(
            &w,
            &a,
            &HashSet::new(),
            &expanded,
            &HashMap::new(),
            &HashMap::new(),
        ));
        assert_eq!(rows.len(), 9);
        assert!(!rows.iter().any(|r| r.kind == RowKind::More));
    }

    #[test]
    fn live_sessions_are_never_capped() {
        let w = vec![wt(MAIN, true)];
        let a: Vec<Assigned> = (0..8).map(|_| owned(MAIN, SessionKind::Live)).collect();
        let rows = build(&input(
            &w,
            &a,
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        ));
        assert_eq!(rows.len(), 9);
    }

    #[test]
    fn orphans_get_their_own_node_at_the_end() {
        let w = vec![wt(MAIN, true)];
        let a = vec![Assigned {
            owner: NodeId::Orphans,
            session: sess(
                "/home/u/repo/.claude/worktrees/gone",
                "/x",
                SessionKind::Resumable,
            ),
            origin: Some("gone".into()),
        }];
        let rows = build(&input(
            &w,
            &a,
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        ));
        assert_eq!(rows[1].kind, RowKind::Orphans);
        assert_eq!(rows[2].kind, RowKind::Orphan);
        // Each orphan row says which worktree it was written for.
        assert!(rows[2].badge.contains("gone"));
    }

    #[test]
    fn sessions_come_out_newest_first() {
        let w = vec![wt(MAIN, true)];
        let mut old = owned(MAIN, SessionKind::Live);
        old.session.last_activity = 100;
        old.session.title = "older".into();
        let mut new = owned(MAIN, SessionKind::Live);
        new.session.last_activity = 900;
        new.session.title = "newer".into();
        let rows = build(&input(
            &w,
            &[old, new],
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        ));
        assert!(rows[1].label.contains("newer"));
    }

    #[test]
    fn the_worktree_row_carries_the_git_badge_and_the_session_count() {
        let w = vec![wt(MAIN, true)];
        let a = vec![owned(MAIN, SessionKind::Live)];
        let mut statuses = HashMap::new();
        statuses.insert(
            PathBuf::from(MAIN),
            GitStatus {
                changed: 3,
                untracked: 1,
                ahead: 0,
                behind: 0,
                has_upstream: true,
            },
        );
        let rows = build(&input(
            &w,
            &a,
            &HashSet::new(),
            &HashMap::new(),
            &statuses,
            &HashMap::new(),
        ));
        assert_eq!(rows[0].badge, "● 1 live");
        assert_eq!(rows[0].status, "~3 ?1");
    }

    #[test]
    fn with_no_live_session_the_badge_counts_the_transcripts_on_disk() {
        let w = vec![wt(MAIN, true)];
        let mut history = HashMap::new();
        history.insert(PathBuf::from(MAIN), 12);
        let rows = build(&input(
            &w,
            &[],
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            &history,
        ));
        assert_eq!(rows[0].badge, "◌ 12 resumable");
    }
}
