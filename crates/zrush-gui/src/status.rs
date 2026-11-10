//! A session's state as a painted dot rather than a glyph: no font needs
//! to carry it, it sits on the text's middle, and a theme colours it.
//!
//! Green is kept for what works right now. A session that runs but waits
//! for you is a quieter dot; one that can resume is a ring.

use egui::Color32;
use zrush_core::model::{Session, SessionKind};

/// Diameter of the dot, in points.
const SIZE: f32 = 8.0;

/// How a session reads at a glance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    /// Running and at work.
    Busy,
    /// Running, its turn over: waiting for you.
    Idle,
    /// Not running: it can be resumed.
    Resumable,
}

impl Activity {
    pub fn of(s: &Session) -> Self {
        match s.kind {
            SessionKind::Resumable => Self::Resumable,
            SessionKind::Live if s.status.starts_with("busy") => Self::Busy,
            SessionKind::Live => Self::Idle,
        }
    }
}

/// The colour of `a`'s dot, and of what is said about it.
pub fn colour(ui: &egui::Ui, a: Activity) -> Color32 {
    let palette = crate::theme::palette(ui.ctx());
    let live = palette.map_or(ui.visuals().selection.bg_fill, |p| p.live);
    match a {
        Activity::Busy => live,
        Activity::Idle => live.gamma_multiply(0.75),
        Activity::Resumable => palette.map_or(ui.visuals().weak_text_color(), |p| p.resumable),
    }
}

/// Paint the dot for `a` and take its room in the layout.
pub fn dot(ui: &mut egui::Ui, a: Activity) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(SIZE, SIZE), egui::Sense::hover());
    paint(ui.painter(), rect.center(), a, colour(ui, a));
}

/// The dot itself, at `center`: for chips and tabs that lay out their own.
pub fn paint(p: &egui::Painter, center: egui::Pos2, a: Activity, colour: Color32) {
    let radius = SIZE / 2.0 - 0.5;
    match a {
        Activity::Busy => {
            // A halo: what is at work reads at a glance down a long list.
            p.circle_filled(center, radius + 3.0, colour.gamma_multiply(0.22));
            p.circle_filled(center, radius, colour);
        }
        Activity::Idle => {
            p.circle_filled(center, radius - 0.5, colour);
        }
        Activity::Resumable => {
            p.circle_stroke(center, radius, egui::Stroke::new(1.2, colour));
        }
    }
}
