//! Purge mode: mark rows, then remove the lot.

use super::{Action, App, Choice, KeyCode, KeyEvent, Modal, Mode};

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
        let plan = self.purge_plan();
        if plan.is_empty() {
            self.flash("nothing to purge: mark rows with space first");
            return Action::Redraw;
        }
        self.modal = Some(Modal::Confirm {
            title: "Purge".into(),
            body: plan.summary(),
            choices: vec![
                Choice::destructive("Purge, and their other sessions too"),
                Choice::destructive("Purge only what is marked"),
                Choice::new("Cancel"),
            ],
            at: 2,
        });
        Action::Redraw
    }

    /// The marked rows, by core's rules.
    pub(super) fn purge_plan(&self) -> zrush_core::purge::Plan {
        zrush_core::purge::plan(&self.rows, self.marked.iter().copied(), &self.snap)
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
        let plan = a.purge_plan();
        assert_eq!(plan.worktrees, Vec::<PathBuf>::new());
        assert!(plan.skipped_main);
    }

    #[test]
    fn a_running_session_is_never_purged() {
        let mut a = app();
        a.on_key(ctrl('p'));
        a.cursor = 3;
        a.on_key(code(KeyCode::Char(' ')));
        let plan = a.purge_plan();
        assert_eq!(plan.sessions, Vec::new());
        assert_eq!(plan.kept_live, 1);
    }

    #[test]
    fn a_session_marked_with_its_worktree_is_deleted_too() {
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
        let plan = a.purge_plan();
        assert_eq!(plan.worktrees.len(), 1);
        // Removing the worktree leaves its transcripts on disk.
        assert_eq!(plan.sessions.len(), 1);
        assert_eq!(plan.sessions[0].id, "dead2");
    }

    /// Worktree x marked, with a dead session of its own nobody marked.
    fn x_marked_with_an_unmarked_session() -> App {
        let mut a = app();
        let mut s = session("unmarked", SessionKind::Resumable);
        s.cwd = "/repo/.claude/worktrees/x".into();
        a.snap.resumable.push(s);
        a.on_key(ctrl('p'));
        a.marked.insert(2); // worktree x
        a
    }

    fn purged(a: &mut App, answer: usize) -> zrush_core::purge::Plan {
        a.on_key(code(KeyCode::Enter));
        let Some(Modal::Confirm { choices, .. }) = a.modal.clone() else {
            panic!("no confirmation");
        };
        match a.confirmed("Purge", &choices, answer) {
            Action::Purge(plan) => plan,
            other => panic!("not a purge: {other:?}"),
        }
    }

    #[test]
    fn purging_with_the_other_sessions_takes_them_too() {
        let mut a = x_marked_with_an_unmarked_session();
        let plan = purged(&mut a, 0);
        assert!(plan.sessions.iter().any(|d| d.id == "unmarked"));
    }

    #[test]
    fn purging_only_what_is_marked_leaves_the_rest() {
        let mut a = x_marked_with_an_unmarked_session();
        let plan = purged(&mut a, 1);
        assert_eq!(plan.sessions, Vec::new());
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
