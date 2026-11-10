//! The window's small pieces, drawn the same way everywhere: the mark, icon
//! buttons, the one primary button, chips.

use egui::{Color32, CornerRadius, FontId, Response, RichText, Sense, Stroke, Ui, vec2};

use crate::theme::Palette;

/// Room for macOS's window buttons, which sit over the top bar: the
/// window's content runs under its title bar there.
#[cfg(target_os = "macos")]
pub const LEFT_INSET: f32 = 76.0;
#[cfg(not(target_os = "macos"))]
pub const LEFT_INSET: f32 = 0.0;

/// The app's icon, as the Dock shows it: the window's icon, and the source
/// tools/bundle-macos.sh makes the macOS bundle's .icns from.
pub const ICON: &[u8] = include_bytes!("../assets/icon.png");

/// The icon's own square within its file: the file keeps macOS's margin,
/// 10 units of 100 on every side, which has no place inside the window.
const BODY: egui::Rect = egui::Rect::from_min_max(egui::pos2(0.1, 0.1), egui::pos2(0.9, 0.9));

/// The icon as a texture, decoded once. Mipmapped: it is drawn at a
/// twentieth of its size.
fn icon_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let id = egui::Id::new("zrush-icon");
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(t);
    }
    let icon = eframe::icon_data::from_png_bytes(ICON).ok()?;
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [icon.width as usize, icon.height as usize],
        &icon.rgba,
    );
    let options = egui::TextureOptions::LINEAR.with_mipmap_mode(Some(egui::TextureFilter::Linear));
    let t = ctx.load_texture("zrush-icon", image, options);
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    Some(t)
}

/// The app's icon in small, the same as in the Dock.
pub fn mark(ui: &mut Ui, size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    if let Some(t) = icon_texture(ui.ctx()) {
        ui.painter().image(t.id(), rect, BODY, Color32::WHITE);
    }
    response
}

/// A borderless button holding one symbol, framed only on hover.
pub fn icon_button(ui: &mut Ui, icon: &str, hover: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(icon).size(15.0))
            .frame_when_inactive(false)
            .min_size(vec2(28.0, 26.0)),
    )
    .on_hover_text(hover)
}

/// The one button a view wants pressed: filled with the accent.
pub fn primary_button(ui: &mut Ui, label: &str) -> Response {
    let (fill, on) = accent(ui);
    ui.add(
        egui::Button::new(
            RichText::new(label)
                .font(crate::fonts::strong(ui.ctx(), 13.5))
                .color(on),
        )
        .fill(fill)
        .stroke(Stroke::NONE)
        .min_size(vec2(0.0, 28.0)),
    )
}

/// The accent and the colour that reads on it.
pub fn accent(ui: &Ui) -> (Color32, Color32) {
    palette(ui).map_or((ui.visuals().hyperlink_color, Color32::WHITE), |p| {
        (p.accent, on_colour(p.accent))
    })
}

/// White or near-black, whichever reads on `bg`.
pub fn on_colour(bg: Color32) -> Color32 {
    let l = 0.2126 * f32::from(bg.r()) + 0.7152 * f32::from(bg.g()) + 0.0722 * f32::from(bg.b());
    if l > 150.0 {
        Color32::from_rgb(20, 20, 24)
    } else {
        Color32::WHITE
    }
}

pub fn palette(ui: &Ui) -> Option<Palette> {
    crate::theme::palette(ui.ctx())
}

/// A small rounded tag: a fact about a row, in its own colour.
pub fn chip(ui: &mut Ui, text: &str, colour: Color32, hover: &str) -> Response {
    let font = FontId::proportional(11.5);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, colour);
    let size = galley.size() + vec2(12.0, 4.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(255), colour.gamma_multiply(0.14));
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, colour);
    // Painted, so it says what it reads to screen readers itself.
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), text));
    if hover.is_empty() {
        response
    } else {
        response.on_hover_text(hover)
    }
}

/// A chip led by a session's painted dot. Painted, not typed: a glyph
/// from a fallback font sits off the text's middle.
pub fn dot_chip(
    ui: &mut Ui,
    a: crate::status::Activity,
    text: &str,
    colour: Color32,
    hover: &str,
) -> Response {
    let font = FontId::proportional(11.5);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, colour);
    let dot = 8.0;
    let size = vec2(galley.size().x + dot + 5.0 + 16.0, galley.size().y + 4.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(255), colour.gamma_multiply(0.14));
    let c = egui::pos2(rect.left() + 8.0 + dot / 2.0, rect.center().y);
    crate::status::paint(p, c, a, colour);
    let at = egui::pos2(
        c.x + dot / 2.0 + 5.0,
        rect.center().y - galley.size().y / 2.0,
    );
    p.galley(at, galley, colour);
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), text));
    if hover.is_empty() {
        response
    } else {
        response.on_hover_text(hover)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_on_the_accent_stays_readable() {
        assert_eq!(on_colour(Color32::from_rgb(0, 90, 200)), Color32::WHITE);
        assert_ne!(on_colour(Color32::from_rgb(250, 220, 120)), Color32::WHITE);
    }
}
