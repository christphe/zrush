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
        explain(ui, &q.body);
        let labels: Vec<&str> = q.choices.iter().map(|a| a.label()).collect();
        let kinds: Vec<Look> = q
            .choices
            .iter()
            .enumerate()
            .map(|(i, a)| match (i, a) {
                (_, Answer::Cancel) => Look::Plain,
                (0, _) => Look::Primary,
                _ => Look::Plain,
            })
            .collect();
        if let Some(i) = buttons(ui, &labels, &kinds, 0) {
            chosen = Some(q.choices[i]);
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            chosen = Some(Answer::Cancel);
        }
    });
    if let Some(a) = chosen {
        tab.answer(a);
    }
}

/// Before something goes for good: what goes, named apart; the destructive
/// answers filled in red; Cancel, the safe one, in focus.
fn confirm(ctx: &egui::Context, tab: &mut Tab, c: &Confirm) {
    let mut chosen = None;
    let choices = c.choices();
    let cancel = choices.len() - 1;
    modal(ctx, c.title(), |ui| {
        if let Some(subject) = c.subject() {
            named(ui, subject);
            ui.add_space(10.0);
        }
        explain(ui, &c.body());
        // The answer that destroys the most, filled; any milder one before
        // it, outlined.
        let kinds: Vec<Look> = (0..choices.len())
            .map(|i| match i {
                _ if i == cancel => Look::Plain,
                _ if i + 1 == cancel => Look::Danger,
                _ => Look::Caution,
            })
            .collect();
        if let Some(i) = buttons(ui, &choices, &kinds, cancel) {
            chosen = Some(i);
        }
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

/// How wide a question is: room for a session's title, narrow enough to
/// read at a glance.
const WIDTH: f32 = 420.0;

/// Past this many characters, answers stack one per line: two long
/// answers side by side do not fit, and stacked they read as a list.
const SIDE_BY_SIDE: usize = 16;

/// How an answer looks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    /// The one to take, in the accent.
    Primary,
    /// Destroys something, filled in red.
    Danger,
    /// Destroys something, but less than the answer after it: red, outlined.
    Caution,
    Plain,
}

/// A window in the middle of the screen, over everything else.
fn modal(ctx: &egui::Context, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    let v = ctx.global_style().visuals.clone();
    let palette = crate::theme::palette(ctx);
    let fill = palette.map_or(v.window_fill, |p| p.panel_bg);
    let border = palette.map_or(v.window_stroke.color, |p| p.border);
    let frame = egui::Frame::new()
        .fill(fill)
        .stroke(egui::Stroke::new(1.0, border))
        .corner_radius(12)
        .inner_margin(egui::Margin::same(22))
        .shadow(v.popup_shadow);
    egui::Modal::new(egui::Id::new(("question", title)))
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(WIDTH);
            ui.label(egui::RichText::new(title).size(17.0).strong());
            ui.add_space(12.0);
            body(ui);
        });
}

/// The thing a question is about, set apart so it reads first.
fn named(ui: &mut egui::Ui, what: &str) {
    let border = crate::theme::palette(ui.ctx())
        .map_or(ui.visuals().widgets.noninteractive.bg_stroke.color, |p| {
            p.border
        });
    egui::Frame::new()
        .fill(ui.visuals().code_bg_color)
        .stroke(egui::Stroke::new(1.0, border))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(egui::RichText::new(what).strong()).truncate());
        });
}

/// What the answers do, quieter than the thing they do it to.
fn explain(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(ui.visuals().weak_text_color()));
}

/// The answers along the bottom, right-aligned, or stacked when they are
/// long. `focus` is the one a stray Enter gives. Returns the one clicked.
fn buttons(ui: &mut egui::Ui, labels: &[&str], looks: &[Look], focus: usize) -> Option<usize> {
    ui.add_space(18.0);
    let stacked = labels.iter().any(|l| l.chars().count() > SIDE_BY_SIDE);
    let mut clicked = None;
    let mut one = |ui: &mut egui::Ui, i: usize, width: f32| {
        let b = ui.add_sized([width, 32.0], answer(ui, labels[i], looks[i]));
        if i == focus && ui.memory(|m| m.focused().is_none()) {
            b.request_focus();
        }
        if b.clicked() {
            clicked = Some(i);
        }
    };
    if stacked {
        let width = ui.available_width();
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            for i in 0..labels.len() {
                one(ui, i, width);
            }
        });
    } else {
        // Right to left, so the last answer, Cancel, sits at the edge.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for i in (0..labels.len()).rev() {
                one(ui, i, 96.0);
            }
        });
    }
    clicked
}

fn answer(ui: &egui::Ui, label: &str, look: Look) -> egui::Button<'static> {
    let v = ui.visuals();
    let text = egui::RichText::new(label.to_string());
    match look {
        Look::Plain => egui::Button::new(text),
        Look::Danger => egui::Button::new(text.color(egui::Color32::WHITE).strong())
            .fill(v.error_fg_color.gamma_multiply(0.85)),
        Look::Caution => egui::Button::new(text.color(v.error_fg_color))
            .fill(egui::Color32::TRANSPARENT)
            .stroke(egui::Stroke::new(1.0, v.error_fg_color.gamma_multiply(0.7))),
        Look::Primary => {
            let accent = crate::theme::palette(ui.ctx()).map_or(v.hyperlink_color, |p| p.accent);
            let on = crate::theme::palette(ui.ctx()).map_or(egui::Color32::WHITE, |p| p.window_bg);
            egui::Button::new(text.color(on).strong()).fill(accent)
        }
    }
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
        assert!(h.query_by_label("side").is_some(), "the worktree is named");
        assert!(
            h.query_by_label("Its folder is removed. It holds 2 sessions.")
                .is_some()
        );
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
