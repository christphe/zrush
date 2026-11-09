//! The questions a tab can put up: core's misfiled and orphan questions,
//! the branch a new worktree gets, and the confirmation before anything
//! goes for good.

use zrush_core::app::BranchKind;
use zrush_core::intent::{Answer, EXISTING_BRANCH, NEW_BRANCH, Question};

use crate::tab::{Confirm, Pending, Tab};

/// How many matching branches the name field offers at once.
const SUGGESTIONS: usize = 8;

/// Put up whatever question `tab` holds. Nothing else in the window takes
/// a click while it is up.
pub fn show(ctx: &egui::Context, tab: &mut Tab) {
    match tab.pending.clone() {
        None => {}
        Some(Pending::Ask(q)) => ask(ctx, tab, &q),
        Some(Pending::Confirm(c)) => confirm(ctx, tab, &c),
        Some(Pending::Branch { kind: None, .. }) => branch_kind(ctx, tab),
        Some(Pending::Branch {
            kind: Some(kind), ..
        }) => branch_name(ctx, tab, kind),
    }
}

/// One of core's questions, its first answer in focus.
fn ask(ctx: &egui::Context, tab: &mut Tab, q: &Question) {
    let mut chosen = None;
    modal(ctx, &q.title, |ui| {
        ui.label(&q.body);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            for (i, a) in q.choices.iter().enumerate() {
                let b = ui.button(a.label());
                if i == 0
                    && !ui.memory(|m| m.has_focus(b.id))
                    && ui.memory(|m| m.focused().is_none())
                {
                    b.request_focus();
                }
                if b.clicked() {
                    chosen = Some(*a);
                }
            }
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            chosen = Some(Answer::Cancel);
        }
    });
    if let Some(a) = chosen {
        tab.answer(a);
    }
}

/// Before something goes for good: the destructive answers in red, and
/// Cancel, the safe one, in focus.
fn confirm(ctx: &egui::Context, tab: &mut Tab, c: &Confirm) {
    let mut chosen = None;
    let choices = c.choices();
    let cancel = choices.len() - 1;
    modal(ctx, c.title(), |ui| {
        ui.label(c.body());
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            let danger = ui.visuals().error_fg_color;
            for (i, label) in choices.iter().enumerate() {
                let text = if i == cancel {
                    egui::RichText::new(*label)
                } else {
                    egui::RichText::new(*label).color(danger)
                };
                let b = ui.button(text);
                // The safe answer is the one a stray Enter gives.
                if i == cancel && ui.memory(|m| m.focused().is_none()) {
                    b.request_focus();
                }
                if b.clicked() {
                    chosen = Some(i);
                }
            }
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            chosen = Some(cancel);
        }
    });
    if let Some(i) = chosen {
        tab.confirm(i);
    }
}

/// A new branch or an existing one, asked before anything is typed: the
/// same name means two different things.
fn branch_kind(ctx: &egui::Context, tab: &mut Tab) {
    let moving = matches!(
        &tab.pending,
        Some(Pending::Branch {
            hand_over: Some(_),
            ..
        })
    );
    let mut picked: Option<Option<BranchKind>> = None;
    modal(ctx, "New worktree", |ui| {
        if moving {
            ui.label("The session moves to it.");
        }
        ui.label("Which branch does it get?");
        ui.horizontal(|ui| {
            if ui.button(NEW_BRANCH).clicked() {
                picked = Some(Some(BranchKind::New));
            }
            if ui.button(EXISTING_BRANCH).clicked() {
                picked = Some(Some(BranchKind::Existing));
            }
            if ui.button(Answer::Cancel.label()).clicked() {
                picked = Some(None);
            }
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            picked = Some(None);
        }
    });
    match picked {
        Some(Some(k)) => {
            if let Some(Pending::Branch { kind, .. }) = &mut tab.pending {
                *kind = Some(k);
            }
        }
        Some(None) => tab.pending = None,
        None => {}
    }
}

/// The branch's name; for an existing one, the branches that match.
fn branch_name(ctx: &egui::Context, tab: &mut Tab, kind: BranchKind) {
    let branches = tab.snap.branches.clone();
    let mut go = false;
    let mut cancel = false;
    modal(ctx, "New worktree", |ui| {
        let Some(Pending::Branch { name, .. }) = &mut tab.pending else {
            return;
        };
        let field = ui.text_edit_singleline(name);
        field.request_focus();
        if kind == BranchKind::Existing {
            let needle = name.to_lowercase();
            for b in branches
                .iter()
                .filter(|b| b.to_lowercase().contains(&needle))
                .take(SUGGESTIONS)
            {
                if ui.selectable_label(false, b).clicked() {
                    name.clone_from(b);
                }
            }
        }
        ui.horizontal(|ui| {
            go = ui.button("Create").clicked()
                || (field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            cancel = ui.button(Answer::Cancel.label()).clicked()
                || ui.input(|i| i.key_pressed(egui::Key::Escape));
        });
    });
    if cancel {
        tab.pending = None;
    } else if go {
        tab.create_branch();
    }
}

/// A small window in the middle of the screen, over everything else.
fn modal(ctx: &egui::Context, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Modal::new(egui::Id::new(("question", title))).show(ctx, |ui| {
        ui.set_max_width(460.0);
        ui.heading(title);
        ui.add_space(6.0);
        body(ui);
    });
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use zrush_core::intent::Answer;

    use super::*;
    use crate::tab::tests::{loaded_with_orphan, orphan_question};

    #[test]
    fn the_dialog_shows_cores_choices() {
        let td = tempfile::TempDir::new().unwrap();
        let (mut tab, _) = loaded_with_orphan(&td);
        let q = orphan_question(&mut tab);
        let choices = q.choices.clone();
        tab.ask(q);
        let mut h = Harness::builder()
            .with_size([900.0, 600.0])
            .build_ui_state(|ui, t: &mut Tab| show(ui.ctx(), t), tab);
        h.run_steps(3);
        for a in choices {
            assert!(h.query_by_label(a.label()).is_some(), "{}", a.label());
        }
        h.get_by_label(Answer::Cancel.label()).click();
        h.run_steps(2);
        assert!(h.state().pending.is_none());
    }

    #[test]
    fn a_confirmation_shows_its_answers_and_cancel_closes_it() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = crate::testing::scratch_repo(&td);
        let mut tab = crate::testing::open_tab(
            &td,
            &repo,
            Box::new(zrush_core::host::RecordingHost::default()),
        );
        tab.pending = Some(Pending::Confirm(crate::tab::Confirm::RemoveWorktree {
            path: repo.join("../side"),
            name: "side".into(),
            sessions: 2,
        }));
        let mut h = Harness::builder()
            .with_size([900.0, 600.0])
            .build_ui_state(|ui, t: &mut Tab| show(ui.ctx(), t), tab);
        h.run_steps(3);
        assert!(h.query_by_label("side has 2 session(s).").is_some());
        for a in [
            "Remove the worktree, keep the sessions",
            "Remove it and delete the sessions too",
        ] {
            assert!(h.query_by_label(a).is_some(), "{a}");
        }
        h.get_by_label("Cancel").click();
        h.run_steps(2);
        assert!(h.state().pending.is_none());
    }
}
