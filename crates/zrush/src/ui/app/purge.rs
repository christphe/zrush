//! Purge mode: mark rows, then remove the lot.

use super::{Action, App, Choice, KeyCode, KeyEvent, Modal, Mode, NodeId, PathBuf, RowKind};

impl App {
    pub(super) fn purge_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => {
                self.leave_purge();
                Action::Reload
            }
            KeyCode::Char(' ') => {
                if let Some(&i) = self.visible.get(self.cursor)
                    && !self.marked.insert(i)
                {
                    self.marked.remove(&i);
                }
                self.move_by(1);
                Action::Redraw
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_by(-1);
                Action::Redraw
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_by(1);
                Action::Redraw
            }
            KeyCode::Enter => self.ask_purge(),
            _ => Action::None,
        }
    }

    pub(super) fn leave_purge(&mut self) {
        self.mode = Mode::Normal;
        self.marked.clear();
        self.show_all = false;
    }

    pub(super) fn ask_purge(&mut self) -> Action {
        let (worktrees, sessions, kept_live, skipped_main) = self.purge_plan();
        if worktrees.is_empty() && sessions.is_empty() {
            self.flash("nothing to purge: mark rows with space first");
            return Action::Redraw;
        }
        let mut note = String::new();
        if kept_live > 0 {
            use std::fmt::Write as _;
            let _ = write!(note, "\n{kept_live} running session(s) kept.");
        }
        if skipped_main > 0 {
            note.push_str("\nThe main worktree is skipped.");
        }
        self.modal = Some(Modal::Confirm {
            title: "Purge".into(),
            body: format!(
                "{} worktree(s) and {} session(s).{note}",
                worktrees.len(),
                sessions.len()
            ),
            choices: vec![
                Choice::destructive("Purge, and their other sessions too"),
                Choice::destructive("Purge only what is marked"),
                Choice::new("Cancel"),
            ],
            at: 2,
        });
        Action::Redraw
    }

    /// Marked rows into a plan. A session whose worktree is also marked
    /// goes with the worktree, so the count never says it twice.
    pub(super) fn purge_plan(&self) -> (Vec<PathBuf>, Vec<(String, String)>, usize, usize) {
        let main = self
            .snap
            .worktrees
            .iter()
            .find(|w| w.is_main)
            .map(|w| w.path.clone());
        let mut worktrees = Vec::new();
        let mut sessions: Vec<(String, String, Option<PathBuf>)> = Vec::new();
        let mut kept_live = 0;
        let mut skipped_main = 0;

        for &i in &self.marked {
            let Some(row) = self.rows.get(i) else {
                continue;
            };
            match row.kind {
                RowKind::Worktree => {
                    let NodeId::Worktree(p) = &row.node else {
                        continue;
                    };
                    if Some(p) == main.as_ref() {
                        skipped_main += 1;
                    } else {
                        worktrees.push(p.clone());
                    }
                }
                RowKind::Session | RowKind::Orphan => {
                    let Some(id) = row.session_id.clone() else {
                        continue;
                    };
                    if self.is_running(&id) {
                        kept_live += 1;
                        continue;
                    }
                    let agent = self
                        .snap
                        .live
                        .iter()
                        .chain(self.snap.resumable.iter())
                        .find(|s| s.id == id)
                        .map_or_else(String::new, |s| s.agent.to_string());
                    let owner = match &row.node {
                        NodeId::Worktree(p) => Some(p.clone()),
                        NodeId::Orphans => None,
                    };
                    sessions.push((agent, id, owner));
                }
                RowKind::Orphans | RowKind::More => {}
            }
        }
        let sessions = sessions
            .into_iter()
            .filter(|(_, _, owner)| !owner.as_ref().is_some_and(|p| worktrees.contains(p)))
            .map(|(a, i, _)| (a, i))
            .collect();
        (worktrees, sessions, kept_live, skipped_main)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::*;
    use zrush_core::model::SessionKind;

    #[test]
    fn purge_mode_unfolds_everything_first() {
        let mut a = app();
        a.collapsed.insert(NodeId::Worktree("/repo".into()));
        assert_eq!(a.on_key(ctrl('p')), Action::ReloadUncapped);
        assert_eq!(a.mode, Mode::Purge);
        assert!(
            a.collapsed.is_empty(),
            "marking rows you cannot see would be a trap"
        );
        assert!(a.show_all);
    }

    #[test]
    fn space_marks_a_row_and_moves_on() {
        let mut a = app();
        a.on_key(ctrl('p'));
        a.on_key(code(KeyCode::Char(' ')));
        assert!(a.marked.contains(&0));
        assert_eq!(a.cursor, 1);
    }

    #[test]
    fn space_again_unmarks_it() {
        let mut a = app();
        a.on_key(ctrl('p'));
        a.on_key(code(KeyCode::Char(' ')));
        a.on_key(code(KeyCode::Up));
        a.on_key(code(KeyCode::Char(' ')));
        assert!(a.marked.is_empty());
    }

    #[test]
    fn purging_nothing_says_so_instead_of_asking() {
        let mut a = app();
        a.on_key(ctrl('p'));
        a.on_key(code(KeyCode::Enter));
        assert!(a.modal.is_none());
        assert!(a.flash.unwrap().contains("mark rows"));
    }

    #[test]
    fn the_main_worktree_is_skipped_by_a_purge() {
        let mut a = app();
        a.on_key(ctrl('p'));
        a.on_key(code(KeyCode::Char(' '))); // the main worktree
        let (worktrees, _, _, skipped) = a.purge_plan();
        assert_eq!(worktrees, Vec::<PathBuf>::new());
        assert_eq!(skipped, 1);
    }

    #[test]
    fn a_running_session_is_never_purged() {
        let mut a = app();
        a.on_key(ctrl('p'));
        a.cursor = 3;
        a.on_key(code(KeyCode::Char(' ')));
        let (_, sessions, kept_live, _) = a.purge_plan();
        assert_eq!(sessions, Vec::<(String, String)>::new());
        assert_eq!(kept_live, 1);
    }

    #[test]
    fn a_session_going_with_its_worktree_is_not_counted_twice() {
        let mut a = app();
        a.on_key(ctrl('p'));
        a.cursor = 2;
        a.on_key(code(KeyCode::Char(' '))); // worktree x
        a.on_key(code(KeyCode::Char(' '))); // its live session, which is kept
        // Now a dead session under a marked worktree.
        a.snap
            .resumable
            .push(session("dead2", SessionKind::Resumable));
        let mut rows = a.rows.clone();
        rows.push(row(
            RowKind::Session,
            NodeId::Worktree("/repo/.claude/worktrees/x".into()),
            Some("dead2"),
            "old",
        ));
        a.set_rows(rows);
        a.marked.insert(4);
        let (worktrees, sessions, ..) = a.purge_plan();
        assert_eq!(worktrees.len(), 1);
        assert!(sessions.is_empty(), "it goes with the worktree");
    }

    #[test]
    fn escaping_purge_mode_clears_the_marks() {
        let mut a = app();
        a.on_key(ctrl('p'));
        a.on_key(code(KeyCode::Char(' ')));
        a.on_key(code(KeyCode::Esc));
        assert_eq!(a.mode, Mode::Normal);
        assert!(a.marked.is_empty());
        assert!(!a.show_all);
    }
}
