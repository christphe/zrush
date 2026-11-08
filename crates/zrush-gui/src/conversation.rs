//! The right pane: the conversation of the selected session, or the git
//! state of the selected worktree.

use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use zrush_core::agent::Role;
use zrush_core::intent::Question;
use zrush_core::loader::Preview;
use zrush_core::model::tree::RowKind;

use crate::tab::Tab;

/// A conversation reads best in a column, not across a wide window.
const COLUMN: f32 = 760.0;

pub fn show(ui: &mut egui::Ui, tab: &mut Tab, md: &mut CommonMarkCache) -> Option<Question> {
    let Some(row) = tab.selected_row().cloned() else {
        ui.centered_and_justified(|ui| ui.weak("Select a session or a worktree"));
        return None;
    };

    let mut asked = None;
    ui.horizontal(|ui| {
        let title = if row.kind == RowKind::Worktree {
            row.label.trim().to_string()
        } else {
            row.search.clone()
        };
        ui.heading(title);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Agent in a terminal").clicked() {
                tab.run_agent(&row);
            }
            if ui.button("Open in IDE").clicked() {
                asked = tab.enter(&row);
            }
        });
    });
    ui.weak(&row.location);
    ui.separator();

    // A new selection starts at its end: the last thing said is what tells
    // you where the session got to.
    let scroll_id = egui::Id::new(("conversation", tab.selected.as_deref().unwrap_or("")));
    egui::ScrollArea::vertical()
        .id_salt(scroll_id)
        .auto_shrink(false)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            ui.set_max_width(COLUMN.min(ui.available_width()));
            match tab.preview() {
                Preview::Empty => {
                    ui.spinner();
                }
                Preview::Worktree(lines) => {
                    for l in lines {
                        ui.monospace(l);
                    }
                }
                Preview::Conversation(turns) => {
                    for (i, t) in turns.iter().enumerate() {
                        ui.add_space(10.0);
                        let who = match t.role {
                            Role::User => "You",
                            Role::Assistant => "Agent",
                        };
                        ui.weak(egui::RichText::new(who).small());
                        ui.push_id(i, |ui| {
                            CommonMarkViewer::new().show(ui, md, &t.text);
                        });
                    }
                    ui.add_space(10.0);
                }
            }
        });
    asked
}
