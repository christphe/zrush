//! The left pane: worktrees as foldable headers, their sessions under them.

use zrush_core::intent::Question;
use zrush_core::model::SessionKind;
use zrush_core::model::tree::{Row, RowKind};
use zrush_core::snapshot::preview_key;

use crate::status;
use crate::tab::Tab;

/// What a click in the tree asks for, done after drawing so the rows are
/// not borrowed while the tab changes.
enum Act {
    Select(Row),
    Enter(Row),
}

pub fn show(ui: &mut egui::Ui, tab: &mut Tab) -> Option<Question> {
    ui.horizontal(|ui| {
        if ui.button("⟳ Refresh").clicked() {
            tab.refresh();
        }
        ui.weak(format!("{} worktrees", tab.snap.worktrees.len()));
        if let Some(b) = &tab.busy {
            ui.spinner();
            ui.weak(b.as_str());
        }
    });
    ui.separator();

    let mut acts: Vec<Act> = Vec::new();
    let selected = tab.selected.clone();
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            for (head, children) in groups(tab.rows()) {
                group(ui, tab, head, &children, selected.as_deref(), &mut acts);
            }
        });

    let mut asked = None;
    for act in acts {
        match act {
            Act::Select(r) => tab.select(&r),
            Act::Enter(r) => {
                tab.select(&r);
                if let Some(q) = tab.enter(&r) {
                    asked = Some(q);
                }
            }
        }
    }
    asked
}

/// The flat rows as headers (a worktree, or the orphans) and what is under
/// each.
fn groups(rows: &[Row]) -> Vec<(&Row, Vec<&Row>)> {
    let mut out: Vec<(&Row, Vec<&Row>)> = Vec::new();
    for r in rows {
        if r.is_child() {
            if let Some((_, children)) = out.last_mut() {
                children.push(r);
            }
        } else {
            out.push((r, Vec::new()));
        }
    }
    out
}

fn group(
    ui: &mut egui::Ui,
    tab: &Tab,
    head: &Row,
    children: &[&Row],
    selected: Option<&str>,
    acts: &mut Vec<Act>,
) {
    let live = children
        .iter()
        .any(|c| kind_of(tab, c) == Some(SessionKind::Live));
    let id = ui.make_persistent_id(("node", &head.node));
    let open_by_default = head.label.contains("[root]") || live;
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, open_by_default)
        .show_header(ui, |ui| {
            let picked = selected.is_some() && preview_key(head).as_deref() == selected;
            let label = ui
                .selectable_label(picked, egui::RichText::new(head.label.trim()).strong())
                .on_hover_text(&head.location);
            if label.clicked() {
                acts.push(Act::Select(head.clone()));
            }
            if label.double_clicked() {
                acts.push(Act::Enter(head.clone()));
            }
            let badge = head.badge.trim_start_matches(['●', '◌', ' ']);
            if !badge.is_empty() {
                ui.weak(badge);
            }
            if !head.status.is_empty() {
                ui.weak(head.status.as_str());
            }
            if head.kind == RowKind::Worktree {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("IDE")
                        .on_hover_text("Open this worktree in the editor, fresh conversation")
                        .clicked()
                    {
                        acts.push(Act::Enter(head.clone()));
                    }
                });
            }
        })
        .body(|ui| {
            if children.is_empty() {
                ui.weak("no sessions");
            }
            for c in children {
                child(ui, tab, c, selected, acts);
            }
        });
}

fn child(ui: &mut egui::Ui, tab: &Tab, row: &Row, selected: Option<&str>, acts: &mut Vec<Act>) {
    if row.kind == RowKind::More {
        if ui.link(row.label.trim()).clicked() {
            acts.push(Act::Enter(row.clone()));
        }
        return;
    }
    ui.horizontal(|ui| {
        if let Some(kind) = kind_of(tab, row) {
            status::dot(ui, kind);
        }
        let picked = selected.is_some() && preview_key(row).as_deref() == selected;
        let r = ui.selectable_label(picked, row.search.as_str());
        if r.clicked() {
            acts.push(Act::Select(row.clone()));
        }
        if r.double_clicked() {
            acts.push(Act::Enter(row.clone()));
        }
        ui.weak(row.badge.as_str());
        let status = row.status.trim().trim_start_matches('·').trim();
        if !status.is_empty() {
            let colour = if row.misfiled {
                ui.visuals().warn_fg_color
            } else {
                ui.visuals().weak_text_color()
            };
            ui.colored_label(colour, status);
        }
    });
}

fn kind_of(tab: &Tab, row: &Row) -> Option<SessionKind> {
    let id = row.session_id.as_deref()?;
    tab.snap.session(id).map(|s| s.kind)
}
