//! The right pane: the conversation of the selected session, or the git
//! state of the selected worktree.

use egui::{Align, Frame, Layout, Margin, RichText};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use zrush_core::agent::Role;
use zrush_core::intent::Question;
use zrush_core::loader::Preview;

use crate::tab::Tab;

/// A conversation reads best in a column, not across a wide window.
const COLUMN: f32 = 760.0;

pub fn show(ui: &mut egui::Ui, tab: &mut Tab, md: &mut CommonMarkCache) -> Option<Question> {
    let Some(row) = tab.selected_row().cloned() else {
        empty(ui);
        return None;
    };

    let palette = crate::theme::palette(ui.ctx());
    let muted = ui.visuals().weak_text_color();
    let mut asked = None;
    ui.horizontal(|ui| {
        // A worktree's branch, or a session's title.
        let title = row.search.clone();
        ui.vertical(|ui| {
            ui.label(RichText::new(title).heading().strong());
            let home = std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(std::path::PathBuf::from);
            let path = zrush_core::paths::shorten(&row.location, home.as_deref());
            if !path.is_empty() {
                ui.label(RichText::new(path).small().color(muted));
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("Agent in a terminal").clicked() {
                tab.run_agent(&row);
            }
            let accent = palette.map_or(ui.visuals().hyperlink_color, |p| p.accent);
            let on_accent = palette.map_or(egui::Color32::WHITE, |p| p.window_bg);
            let open = egui::Button::new(RichText::new("Open in IDE").color(on_accent).strong())
                .fill(accent);
            if ui.add(open).clicked() {
                asked = tab.enter(&row);
            }
        });
    });
    ui.add_space(6.0);
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
                    ui.add_space(24.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Reading…").color(muted));
                    });
                }
                Preview::Worktree(lines) => {
                    ui.add_space(8.0);
                    Frame::new()
                        .fill(ui.visuals().code_bg_color)
                        .corner_radius(8)
                        .inner_margin(Margin::symmetric(14, 12))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            for l in lines {
                                ui.label(RichText::new(l).monospace());
                            }
                        });
                }
                Preview::Conversation(turns) => {
                    for (i, t) in turns.iter().enumerate() {
                        ui.add_space(14.0);
                        ui.push_id(i, |ui| turn(ui, t, md, palette));
                    }
                    ui.add_space(16.0);
                }
            }
        });
    asked
}

/// What you said sits in a card; what the agent answered reads as the
/// page itself.
fn turn(
    ui: &mut egui::Ui,
    t: &zrush_core::agent::Turn,
    md: &mut CommonMarkCache,
    palette: Option<crate::theme::Palette>,
) {
    let muted = ui.visuals().weak_text_color();
    match t.role {
        Role::User => {
            let colour = palette.map_or(muted, |p| p.user_turn);
            let fill = palette.map_or(ui.visuals().faint_bg_color, |p| p.panel_bg);
            Frame::new()
                .fill(fill)
                .corner_radius(10)
                .inner_margin(Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(RichText::new("You").small().strong().color(colour));
                    ui.add_space(2.0);
                    CommonMarkViewer::new().show(ui, md, &t.text);
                });
        }
        Role::Assistant => {
            let colour = palette.map_or(muted, |p| p.agent_turn);
            Frame::new()
                .inner_margin(Margin::symmetric(14, 0))
                .show(ui, |ui| {
                    ui.label(RichText::new("Agent").small().strong().color(colour));
                    ui.add_space(2.0);
                    CommonMarkViewer::new().show(ui, md, &t.text);
                });
        }
    }
}

/// Nothing selected yet: say what this pane is for.
fn empty(ui: &mut egui::Ui) {
    let muted = ui.visuals().weak_text_color();
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.35);
        ui.label(RichText::new("Nothing selected").heading().color(muted));
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "Pick a session to read its conversation, or a worktree to see its git state.",
            )
            .color(muted),
        );
    });
}
