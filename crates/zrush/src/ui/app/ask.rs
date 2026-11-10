//! Enter on a row, and the misfiled and orphan questions core asks about it.

use super::{
    Action, App, Choice, EXISTING_BRANCH, Intent, MISFILED, Modal, NEW_BRANCH, Outcome, Pending,
    RESUME, UPDATE, UPDATE_NOW, UPDATE_SKIP, intent,
};

impl App {
    pub(super) fn enter(&mut self) -> Action {
        let Some(row) = self.current() else {
            return Action::None;
        };
        match intent::enter(row, &self.snap) {
            Intent::Nothing => Action::None,
            // Not an action: it raises this node's cap and redraws, so enter
            // after enter keeps unfolding.
            Intent::ShowMore(node) => {
                let cur = self
                    .expanded
                    .get(&node)
                    .copied()
                    .unwrap_or(self.resumable_max);
                self.expanded.insert(node, cur + self.more_step);
                Action::ReloadUncapped
            }
            Intent::Open { worktree, bind } => Action::Open { worktree, bind },
            Intent::Ask(q) => self.ask(q),
        }
    }

    /// Asked before anything is typed: on the same word, "new" cuts a
    /// branch and "existing" opens one that is already there, possibly
    /// someone else's. A completion list under an input that also accepts
    /// new names cannot say which of the two it is offering.
    pub(super) fn ask_branch_kind(&mut self) {
        // Keeps whatever ask_where put there; ctrl-w resets it first.
        self.modal = Some(Modal::Confirm {
            title: "New worktree".into(),
            body: "Which branch does it get?".into(),
            choices: vec![
                Choice::new(NEW_BRANCH),
                Choice::new(EXISTING_BRANCH),
                Choice::new("Cancel"),
            ],
            at: 0,
        });
    }

    /// Put core's question up, remembering what it is about.
    pub(super) fn ask(&mut self, q: intent::Question) -> Action {
        let (session, worktree, at) = match &q.topic {
            intent::Topic::Misfiled {
                agent,
                id,
                worktree,
            } => (
                Some((agent.clone(), id.clone())),
                Some(worktree.clone()),
                None,
            ),
            intent::Topic::Orphan { agent, id, gone } => {
                (Some((agent.clone(), id.clone())), None, gone.clone())
            }
        };
        self.modal = Some(Modal::Confirm {
            title: q.title.clone(),
            body: q.body.clone(),
            choices: q.choices.iter().map(|a| Choice::new(a.label())).collect(),
            at: 0,
        });
        self.pending = Pending {
            session,
            worktree,
            at,
            question: Some(q),
        };
        Action::Redraw
    }

    pub(super) fn confirmed(&mut self, title: &str, choices: &[Choice], at: usize) -> Action {
        let last = choices.len() - 1;
        if at == last {
            // Cancel: the question is over, and nothing it captured may
            // outlive it — a path kept here would land the next worktree
            // somewhere nobody asked for.
            self.pending = Pending::default();
            if title == "Purge" {
                self.leave_purge();
                return Action::Reload;
            }
            // Later: out of the header until the next start.
            if title == UPDATE {
                self.update = None;
            }
            return Action::Redraw;
        }
        match title {
            RESUME | MISFILED => {
                let picked = choices.get(at).map(|c| c.label.clone()).unwrap_or_default();
                let (Some(q), Some(a)) = (
                    self.pending.question.clone(),
                    intent::Answer::from_label(&picked),
                ) else {
                    return Action::Redraw;
                };
                match intent::answer(&q, a, &self.snap) {
                    Outcome::Nothing => Action::Redraw,
                    Outcome::Open { worktree, bind } => Action::Open { worktree, bind },
                    Outcome::Adopt {
                        agent,
                        id,
                        worktree,
                    } => Action::AdoptSession {
                        agent,
                        id,
                        worktree,
                    },
                    // Which branch it gets is a second question — git does
                    // not remember the one the removed worktree had, and
                    // `pending.at` already holds where it must land.
                    Outcome::NeedBranch { at, hand_over } => {
                        self.pending.at = at;
                        self.pending.session = Some(hand_over);
                        self.ask_branch_kind();
                        Action::Redraw
                    }
                }
            }
            "New worktree" => {
                let existing = choices.get(at).is_some_and(|c| c.label == EXISTING_BRANCH);
                self.open_branch_input(existing);
                Action::Redraw
            }
            UPDATE => {
                let picked = choices.get(at).map(|c| c.label.clone()).unwrap_or_default();
                let Some(tag) = self.update.take() else {
                    return Action::Redraw;
                };
                if picked == UPDATE_NOW {
                    Action::Update(tag)
                } else if picked == UPDATE_SKIP {
                    Action::SkipUpdate(tag)
                } else {
                    Action::Redraw
                }
            }
            "Delete session" => {
                self.pending
                    .session
                    .clone()
                    .map_or(Action::Redraw, |(agent, id)| Action::DeleteSession {
                        agent,
                        id,
                        worktree: self.pending.worktree.clone(),
                    })
            }
            "Remove worktree" => self
                .pending
                .worktree
                .clone()
                .map_or(Action::Redraw, |path| Action::RemoveWorktree {
                    path,
                    with_sessions: choices.len() == 3 && at == 1,
                }),
            "Purge" => {
                let mut plan = self.purge_plan();
                // The first answer takes the marked worktrees' other
                // sessions too; the second only what is marked.
                if at == 0 {
                    plan = plan.with_others(&self.snap);
                }
                self.leave_purge();
                Action::Purge(plan)
            }
            _ => Action::Redraw,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::*;
    use zrush_core::model::{Session, SessionKind};

    #[test]
    fn enter_on_a_worktree_opens_it_with_no_binding() {
        let mut a = app();
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::Open {
                worktree: "/repo".into(),
                bind: None
            }
        );
    }

    #[test]
    fn enter_on_a_session_binds_it_first() {
        let mut a = app();
        a.on_key(code(KeyCode::Down));
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::Open {
                worktree: "/repo".into(),
                bind: Some(("claude".into(), "dead".into()))
            }
        );
    }

    #[test]
    fn enter_on_a_more_row_raises_the_cap_instead_of_acting() {
        let mut a = app();
        a.set_rows(vec![
            row(
                RowKind::Worktree,
                NodeId::Worktree("/repo".into()),
                None,
                "main",
            ),
            row(
                RowKind::More,
                NodeId::Worktree("/repo".into()),
                None,
                "[…more]",
            ),
        ]);
        a.on_key(code(KeyCode::Down));
        assert_eq!(a.on_key(code(KeyCode::Enter)), Action::ReloadUncapped);
        assert_eq!(a.expanded.get(&NodeId::Worktree("/repo".into())), Some(&25));
    }

    #[test]
    fn enter_on_a_misfiled_session_asks_instead_of_opening_an_empty_panel() {
        let mut a = with_misfiled();
        assert_eq!(a.on_key(code(KeyCode::Enter)), Action::Redraw);
        match a.modal.as_ref().unwrap() {
            Modal::Confirm {
                title, choices, at, ..
            } => {
                assert_eq!(title, MISFILED);
                assert_eq!(choices[*at].label, MISFILED_FILE);
                let labels: Vec<&str> = choices.iter().map(|c| c.label.as_str()).collect();
                assert_eq!(labels, vec![MISFILED_FILE, MISFILED_ANYWAY, "Cancel"]);
            }
            other => panic!("wrong modal: {other:?}"),
        }
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::AdoptSession {
                agent: "claude".into(),
                id: "dead".into(),
                worktree: "/repo".into(),
            }
        );
    }

    /// The second answer is what enter did before, now named for what it
    /// really does rather than promising a resume.
    #[test]
    fn opening_it_anyway_is_the_old_behaviour_under_its_own_name() {
        let mut a = with_misfiled();
        a.on_key(code(KeyCode::Enter));
        a.on_key(code(KeyCode::Down));
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::Open {
                worktree: "/repo".into(),
                bind: Some(("claude".into(), "dead".into())),
            }
        );
    }

    /// A session filed where it is listed must not gain a question.
    #[test]
    fn enter_on_an_ordinary_session_still_just_opens_it() {
        let mut a = app();
        a.on_key(code(KeyCode::Down));
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::Open {
                worktree: "/repo".into(),
                bind: Some(("claude".into(), "dead".into())),
            }
        );
    }

    /// Every probe rebuilds the rows while a modal is up. Reading the
    /// cursor again at confirm time deleted whatever had slid under it.
    #[test]
    fn a_rebuild_under_the_question_does_not_move_its_target() {
        let mut a = app();
        a.snap.resumable.push(Session {
            agent: "claude",
            id: "other".into(),
            title: "someone else".into(),
            status: "resumable 1d".into(),
            cwd: "/repo".into(),
            launch_cwd: "/repo".into(),
            kind: SessionKind::Resumable,
            last_activity: 0,
        });
        a.on_key(code(KeyCode::Down)); // onto "old work", session dead
        a.on_key(ctrl('d'));

        // A probe lands: the row that was asked about is gone, and another
        // session takes its place under the cursor.
        a.set_rows(vec![
            row(
                RowKind::Worktree,
                NodeId::Worktree("/repo".into()),
                None,
                "main",
            ),
            row(
                RowKind::Session,
                NodeId::Worktree("/repo".into()),
                Some("other"),
                "someone else",
            ),
        ]);

        a.on_key(code(KeyCode::Up)); // onto "Delete it"
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::DeleteSession {
                agent: "claude".into(),
                id: "dead".into(),
                worktree: Some("/repo".into()),
            },
            "it must still be the session the question named"
        );
    }

    /// Same for a worktree: the answer belongs to the row that was asked
    /// about, not to whatever the cursor ended up on.
    #[test]
    fn removing_a_worktree_keeps_the_one_the_question_named() {
        let mut a = app();
        a.on_key(ctrl('d')); // on main [root]
        a.set_rows(vec![row(
            RowKind::Worktree,
            NodeId::Worktree("/repo/.claude/worktrees/x".into()),
            None,
            "x",
        )]);
        a.on_key(code(KeyCode::Up));
        match a.on_key(code(KeyCode::Enter)) {
            Action::RemoveWorktree { path, .. } => assert_eq!(path, PathBuf::from("/repo")),
            other => panic!("wrong action: {other:?}"),
        }
    }

    /// Cancelling the branch question used to leave the orphan's path
    /// behind, and the next ctrl-w built its worktree there.
    #[test]
    fn cancelling_a_recreate_does_not_place_the_next_worktree() {
        let mut a = with_orphan();
        a.on_key(code(KeyCode::Enter)); // the question
        a.on_key(code(KeyCode::Enter)); // Recreate its worktree
        a.on_key(code(KeyCode::Esc)); // …then think better of it
        assert!(a.modal.is_none());

        a.cursor = 0;
        ask_new_branch(&mut a);
        for c in "feat".chars() {
            a.on_key(key(c));
        }
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::CreateWorktree {
                name: "feat".into(),
                kind: BranchKind::New,
                at: None,
                hand_over: None,
            }
        );
    }

    /// The same, through Cancel rather than esc.
    #[test]
    fn cancelling_the_branch_question_forgets_the_path_too() {
        let mut a = with_orphan();
        a.on_key(code(KeyCode::Enter));
        a.on_key(code(KeyCode::Enter)); // Recreate its worktree
        a.on_key(code(KeyCode::Up)); // onto Cancel
        a.on_key(code(KeyCode::Enter));

        a.cursor = 0;
        ask_new_branch(&mut a);
        for c in "feat".chars() {
            a.on_key(key(c));
        }
        match a.on_key(code(KeyCode::Enter)) {
            Action::CreateWorktree { at, .. } => assert_eq!(at, None),
            other => panic!("wrong action: {other:?}"),
        }
    }

    #[test]
    fn enter_on_an_orphan_asks_where_instead_of_doing_nothing() {
        let mut a = with_orphan();
        assert_eq!(a.on_key(code(KeyCode::Enter)), Action::Redraw);
        match a.modal.as_ref().unwrap() {
            Modal::Confirm {
                title,
                body,
                choices,
                at,
            } => {
                assert_eq!(title, RESUME);
                // Putting the directory back is the answer that changes
                // nothing else, so it is the one under the cursor.
                assert_eq!(choices[*at].label, RESUME_RECREATE);
                let labels: Vec<&str> = choices.iter().map(|c| c.label.as_str()).collect();
                assert_eq!(labels, vec![RESUME_RECREATE, RESUME_ROOT, "Cancel"]);
                // It names the directory it would put back.
                assert!(body.contains("gone"), "{body}");
            }
            other => panic!("wrong modal: {other:?}"),
        }
    }

    /// Recreating means the session's own old path, not the one the branch
    /// name would produce: the agent files transcripts by launch directory.
    #[test]
    fn recreating_carries_the_old_path_into_the_branch_question() {
        let mut a = with_orphan();
        a.on_key(code(KeyCode::Enter));
        a.on_key(code(KeyCode::Enter)); // Recreate its worktree
        match a.modal.as_ref().unwrap() {
            Modal::Confirm { title, .. } => assert_eq!(title, "New worktree"),
            other => panic!("wrong modal: {other:?}"),
        }
        a.on_key(code(KeyCode::Enter)); // New branch
        for c in "fix/thing".chars() {
            a.on_key(key(c));
        }
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::CreateWorktree {
                name: "fix/thing".into(),
                kind: BranchKind::New,
                at: Some("/repo/.claude/worktrees/gone".into()),
                hand_over: Some(("claude".into(), "dead".into())),
            }
        );
    }

    /// ctrl-w on an ordinary row still lands where the branch name says.
    #[test]
    fn a_plain_new_worktree_carries_no_path() {
        let mut a = app();
        ask_new_branch(&mut a);
        for c in "feat".chars() {
            a.on_key(key(c));
        }
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::CreateWorktree {
                name: "feat".into(),
                kind: BranchKind::New,
                at: None,
                hand_over: None,
            }
        );
    }

    #[test]
    fn moving_it_to_the_root_refiles_it_there() {
        let mut a = with_orphan();
        a.on_key(code(KeyCode::Enter));
        a.on_key(code(KeyCode::Down));
        assert_eq!(
            a.on_key(code(KeyCode::Enter)),
            Action::AdoptSession {
                agent: "claude".into(),
                id: "dead".into(),
                worktree: "/repo".into(),
            }
        );
    }
}
