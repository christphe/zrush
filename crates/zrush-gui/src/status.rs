//! A session's state as a painted dot rather than a glyph: no font needs
//! to carry it, and a theme colours it.

use zrush_core::model::SessionKind;

/// Diameter of the dot, in points.
const SIZE: f32 = 8.0;

/// Paint the dot for `kind` and take its room in the layout.
pub fn dot(ui: &mut egui::Ui, kind: SessionKind) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(SIZE, SIZE), egui::Sense::hover());
    let center = rect.center();
    let radius = SIZE / 2.0 - 0.5;
    match kind {
        SessionKind::Live => {
            ui.painter()
                .circle_filled(center, radius, ui.visuals().selection.bg_fill);
        }
        SessionKind::Resumable => {
            ui.painter().circle_stroke(
                center,
                radius,
                egui::Stroke::new(1.2, ui.visuals().weak_text_color()),
            );
        }
    }
}
