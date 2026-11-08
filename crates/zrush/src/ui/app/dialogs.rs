//! The confirmations that are not about what a row means: delete, update.

use super::{
    Action, App, Choice, Modal, NodeId, Pending, RowKind, UPDATE, UPDATE_NOW, UPDATE_SKIP,
};

impl App {
    /// `U`, when the header offers a release. Nothing to ask otherwise.
    pub(super) fn ask_update(&mut self) -> Action {
        let Some(tag) = self.update.clone() else {
            return Action::None;
        };
        self.modal = Some(Modal::Confirm {
            title: UPDATE.into(),
            body: format!(
                "zrush {tag} is out; this is {}.

Updating leaves zrush and runs the installer here.
Other zrush windows keep the old one until they restart.",
                zrush_core::update::current()
            ),
            choices: vec![
                Choice::new(UPDATE_NOW),
                Choice::new(UPDATE_SKIP),
                Choice::new("Later"),
            ],
            at: 0,
        });
        Action::Redraw
    }

    pub(super) fn ask_delete(&mut self) -> Action {
        let Some(row) = self.current() else {
            return Action::None;
        };
        match row.kind {
            RowKind::Session | RowKind::Orphan => {
                let Some((agent, id)) = self.session_at_cursor() else {
                    return Action::None;
                };
                if self.is_running(&id) {
                    self.flash("that session is running; stop it first");
                    return Action::Redraw;
                }
                let title = row.label.trim().to_string();
                self.pending = Pending {
                    session: Some((agent, id)),
                    worktree: self.worktree_at_cursor(),
                    at: None,
                    question: None,
                };
                self.modal = Some(Modal::Confirm {
                    title: "Delete session".into(),
                    body: format!("{title}\n\nIts transcript goes for good."),
                    choices: vec![Choice::destructive("Delete it"), Choice::new("Cancel")],
                    at: 1,
                });
                Action::Redraw
            }
            RowKind::Worktree => {
                let Some(path) = self.worktree_at_cursor() else {
                    return Action::None;
                };
                self.pending = Pending {
                    session: None,
                    worktree: Some(path.clone()),
                    at: None,
                    question: None,
                };
                let node = NodeId::Worktree(path.clone());
                let n = self
                    .rows
                    .iter()
                    .filter(|r| r.node == node && r.is_child())
                    .count();
                let name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let choices = if n == 0 {
                    vec![Choice::destructive("Remove it"), Choice::new("Cancel")]
                } else {
                    vec![
                        Choice::new("Remove the worktree, keep the sessions"),
                        Choice::destructive("Remove it and delete the sessions too"),
                        Choice::new("Cancel"),
                    ]
                };
                self.modal = Some(Modal::Confirm {
                    title: "Remove worktree".into(),
                    body: if n == 0 {
                        format!("{name} has no sessions.")
                    } else {
                        format!("{name} has {n} session(s).")
                    },
                    at: choices.len() - 1,
                    choices,
                });
                Action::Redraw
            }
            RowKind::More | RowKind::Orphans => Action::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::*;

    #[test]
    fn the_delete_confirmation_starts_on_the_safe_answer() {
        let mut a = app();
        a.on_key(code(KeyCode::Down));
        a.on_key(ctrl('d'));
        match a.modal.as_ref().unwrap() {
            Modal::Confirm { choices, at, .. } => {
                assert_eq!(*at, choices.len() - 1, "the cursor must start on Cancel");
                assert_eq!(choices[*at].label, "Cancel");
            }
            other => panic!("wrong modal: {other:?}"),
        }
    }

    #[test]
    fn cancelling_a_deletion_does_nothing() {
        let mut a = app();
        a.on_key(code(KeyCode::Down));
        a.on_key(ctrl('d'));
        assert_eq!(a.on_key(code(KeyCode::Enter)), Action::Redraw);
    }

    #[test]
    fn confirming_a_deletion_names_the_session_and_its_worktree() {
        let mut a = app();
        a.on_key(code(KeyCode::Down));
        a.on_key(ctrl('d'));
        a.on_key(code(KeyCode::Up)); // onto "Delete it"
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::DeleteSession {
                agent: "claude".into(),
                id: "dead".into(),
                worktree: Some("/repo".into()),
            }
        );
    }

    #[test]
    fn a_running_session_refuses_deletion_before_it_asks() {
        let mut a = app();
        a.cursor = 3; // the live one
        a.on_key(ctrl('d'));
        assert!(a.modal.is_none());
        assert!(a.flash.unwrap().contains("running"));
    }

    #[test]
    fn removing_a_worktree_offers_keeping_its_sessions() {
        let mut a = app();
        a.on_key(ctrl('d'));
        match a.modal.as_ref().unwrap() {
            Modal::Confirm { choices, .. } => assert_eq!(choices.len(), 3),
            other => panic!("wrong modal: {other:?}"),
        }
    }

    #[test]
    fn a_worktree_with_no_sessions_gets_a_two_answer_dialog() {
        let mut a = app();
        a.set_rows(vec![row(
            RowKind::Worktree,
            NodeId::Worktree("/repo".into()),
            None,
            "main",
        )]);
        a.on_key(ctrl('d'));
        match a.modal.as_ref().unwrap() {
            Modal::Confirm { choices, .. } => assert_eq!(choices.len(), 2),
            other => panic!("wrong modal: {other:?}"),
        }
    }
}

#[cfg(test)]
mod update_tests {
    use super::super::*;

    fn shift_u() -> KeyEvent {
        // What a terminal sends for U: the capital, with shift held.
        KeyEvent::new(KeyCode::Char('U'), KeyModifiers::SHIFT)
    }

    fn enter() -> KeyEvent {
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
    }

    fn down() -> KeyEvent {
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)
    }

    fn offered() -> App {
        let mut a = App::new(20, 5);
        a.update = Some("v9.9.9".into());
        a
    }

    #[test]
    fn u_does_nothing_until_a_release_is_offered() {
        let mut a = App::new(20, 5);
        assert!(matches!(a.on_key(shift_u()), Action::None));
        assert!(a.modal.is_none());
    }

    #[test]
    fn u_asks_with_update_now_under_the_cursor() {
        let mut a = offered();
        a.on_key(shift_u());
        match &a.modal {
            Some(Modal::Confirm {
                title, choices, at, ..
            }) => {
                assert_eq!(title, UPDATE);
                assert_eq!(choices[*at].label, UPDATE_NOW);
            }
            other => panic!("wrong modal: {other:?}"),
        }
    }

    #[test]
    fn update_now_hands_the_tag_over() {
        let mut a = offered();
        a.on_key(shift_u());
        match a.on_key(enter()) {
            Action::Update(tag) => assert_eq!(tag, "v9.9.9"),
            other => panic!("wrong action: {other:?}"),
        }
    }

    #[test]
    fn skip_names_the_tag_and_clears_the_header() {
        let mut a = offered();
        a.on_key(shift_u());
        a.on_key(down());
        match a.on_key(enter()) {
            Action::SkipUpdate(tag) => assert_eq!(tag, "v9.9.9"),
            other => panic!("wrong action: {other:?}"),
        }
        assert_eq!(a.update, None);
    }

    #[test]
    fn later_hides_it_without_skipping_anything() {
        let mut a = offered();
        a.on_key(shift_u());
        a.on_key(down());
        a.on_key(down());
        assert!(matches!(a.on_key(enter()), Action::Redraw));
        assert_eq!(a.update, None);
        assert!(a.modal.is_none());
    }
}
