//! The settings window: every setting the terminal's settings screen has,
//! written to the same `config.toml`.
//!
//! Laid out the way a system's settings are: the sections listed on the
//! left, each a heading over a card of rows on the right, a name and a line
//! saying what it does on the left of a row and its control on the right.
//! The themes are shown, not named: a small picture of each.

use std::collections::HashMap;

use egui::{Align, Color32, Layout, RichText, Sense, Stroke, Ui, vec2};
use zrush_core::actions;
use zrush_core::app::Zrush;
use zrush_core::config::{Config, KNOWN_EDITORS, Kind, SETTINGS};

use crate::theme::{Palette, Theme};

/// A change that went through, for the window to pass on to every tab.
pub enum Changed {
    Config(Config),
}

/// What the window holds between frames: whether it is open, the text
/// being typed in each field, and each field's last refusal.
#[derive(Default)]
pub struct SettingsState {
    pub open: bool,
    drafts: HashMap<String, String>,
    errors: HashMap<String, String>,
    /// The config as saved (file, then environment), read when the window
    /// needs it and again after each change. Not the service's live copy:
    /// that one carries the window's own overrides.
    saved: Option<Config>,
    /// The section a click on the left asked to be scrolled to.
    go_to: Option<usize>,
    /// The section at the top of the page, lit on the left.
    current: usize,
}

/// The sections, in order: a title, what they hold, and their settings.
/// Appearance has the themes before its rows.
const SECTIONS: &[(&str, &str, &[&str])] = &[
    ("Appearance", "How the window looks.", &[]),
    (
        "Editor and agent",
        "What opens a worktree, and whose sessions are listed.",
        &["editor", "editor_cmd", "terminal", "agent"],
    ),
    (
        "Sessions",
        "What the tree lists under each worktree.",
        &[
            "titles",
            "status",
            "resumable_max",
            "more_step",
            "resumable_scan",
        ],
    ),
    (
        "Terminal",
        "Read by zrush in a terminal only.",
        &["title_width", "preview_turns"],
    ),
    (
        "Starting up",
        "What happens when zrush opens.",
        &["default_repo", "update_check"],
    ),
];

/// What a setting is called on screen; its `config.toml` name shows on
/// hover.
fn label_for(key: &str) -> &'static str {
    match key {
        "editor" => "Editor",
        "editor_cmd" => "Editor command",
        "terminal" => "Terminal",
        "agent" => "Agent",
        "titles" => "Session titles",
        "status" => "Git status column",
        "resumable_max" => "Sessions per worktree",
        "more_step" => "Show more by",
        "resumable_scan" => "Transcripts scanned",
        "title_width" => "Title width",
        "preview_turns" => "Preview turns",
        "default_repo" => "Default repository",
        "update_check" => "Check for updates",
        _ => "Setting",
    }
}

/// What a setting does, in a line.
fn says(key: &str) -> &'static str {
    match key {
        "editor" => "What Open in IDE runs, in a new window.",
        "editor_cmd" => "A whole command line instead; the folder goes at the end.",
        "terminal" => "How a session resumes in a terminal. Empty: the system's own.",
        "agent" => "Whose sessions are listed.",
        "titles" => "Read each session's title from its transcript.",
        "status" => "Changed, staged and untracked files per worktree. Slow on huge repositories.",
        "resumable_max" => "Past sessions listed under a worktree before \u{201c}more\u{201d}.",
        "more_step" => "How many more one click on \u{201c}more\u{201d} lists.",
        "resumable_scan" => "Newest transcripts read at each refresh; a search reads them all.",
        "title_width" => "Where a long title is cut.",
        "preview_turns" => "How many turns the preview shows.",
        "default_repo" => "Opened when zrush starts outside any repository.",
        "update_check" => "Look for a newer release once a day.",
        _ => "",
    }
}

/// Draw the window when it is open. `z` is the service the change goes
/// through; the window applies what comes back to the other tabs.
pub fn show(
    ctx: &egui::Context,
    state: &mut SettingsState,
    z: &Zrush,
    themes: &[Theme],
) -> Option<Changed> {
    if !state.open {
        return None;
    }
    let mut changed = None;
    // Placed once, then the user's to move and resize: an anchored window
    // can do neither. It opens no taller than the window it sits in.
    let screen = ctx.content_rect();
    let height = (screen.height() - 120.0).clamp(300.0, 680.0);
    let width = 780.0_f32.min(screen.width() - 40.0);
    let v = ctx.global_style().visuals.clone();
    egui::Window::new("Settings")
        .title_bar(false)
        .collapsible(false)
        .resizable(true)
        .constrain(true)
        .default_size([width, height])
        .min_size([520.0, 280.0])
        .default_pos([screen.center().x - width / 2.0, screen.top() + 64.0])
        .frame(
            egui::Frame::window(&ctx.global_style())
                .fill(v.window_fill)
                .corner_radius(14)
                .inner_margin(0),
        )
        .show(ctx, |ui| {
            let cfg = state
                .saved
                .get_or_insert_with(|| Config::load(z.dirs()).unwrap_or_else(|_| z.config()))
                .clone();
            // The first frame measures the window with no bound: what fills
            // the width would claim the whole screen.
            if ui.is_sizing_pass() {
                ui.set_max_size(vec2(width, height));
            }
            if header(ui) {
                state.open = false;
            }
            egui::Panel::left("settings-nav")
                .resizable(false)
                .exact_size(186.0)
                .frame(egui::Frame::new())
                .show(ui, |ui| nav(ui, state));
            egui::CentralPanel::default()
                .frame(egui::Frame::new())
                .show(ui, |ui| {
                    if let Some(c) = page(ui, state, z, &cfg, themes) {
                        changed = Some(c);
                    }
                });
        });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !ctx.egui_wants_keyboard_input() {
        state.open = false;
    }
    if changed.is_some() {
        state.saved = None;
    }
    changed
}

/// The window's own title bar: its name, and the way out. True when that
/// was clicked.
fn header(ui: &mut Ui) -> bool {
    let mut close = false;
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 20,
            right: 12,
            top: 14,
            bottom: 12,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Settings").font(crate::fonts::strong(ui.ctx(), 17.0)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    close = crate::chrome::icon_button(ui, "✕", "Close (Esc)").clicked();
                });
            });
        });
    let line = ui.visuals().widgets.noninteractive.bg_stroke.color;
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, line);
    close
}

/// The sections, down the left: a click scrolls the page to one, and the
/// one at the top of the page is lit.
fn nav(ui: &mut Ui, state: &mut SettingsState) {
    ui.vertical(|ui| {
        ui.set_width(170.0);
        ui.add_space(12.0);
        let (accent, _) = crate::chrome::accent(ui);
        for (i, (title, _, _)) in SECTIONS.iter().enumerate() {
            let (rect, r) = ui.allocate_exact_size(vec2(170.0, 32.0), Sense::click());
            let on = state.current == i;
            let inner = rect.shrink2(vec2(10.0, 2.0));
            if on {
                ui.painter()
                    .rect_filled(inner, 8.0, accent.gamma_multiply(0.14));
            } else if r.hovered() {
                ui.painter().rect_filled(
                    inner,
                    8.0,
                    ui.visuals()
                        .widgets
                        .hovered
                        .weak_bg_fill
                        .gamma_multiply(0.6),
                );
            }
            let colour = if on {
                accent
            } else {
                ui.visuals().text_color()
            };
            let font = if on {
                crate::fonts::strong(ui.ctx(), 13.5)
            } else {
                egui::FontId::proportional(13.5)
            };
            ui.painter().text(
                inner.left_center() + vec2(12.0, 0.0),
                egui::Align2::LEFT_CENTER,
                *title,
                font,
                colour,
            );
            r.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, *title)
            });
            if r.clicked() {
                state.go_to = Some(i);
            }
        }
    });
}

/// Every section, one under the other, in a page that scrolls.
fn page(
    ui: &mut Ui,
    state: &mut SettingsState,
    z: &Zrush,
    cfg: &Config,
    themes: &[Theme],
) -> Option<Changed> {
    let mut changed = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_viewport(ui, |ui, viewport| {
            egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: 24,
                    right: 24,
                    top: 16,
                    bottom: 24,
                })
                .show(ui, |ui| {
                    // Top to bottom, as wide as the page: the row this
                    // page sits in lays out sideways.
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        ui.set_width(ui.available_width());
                        let top = ui.min_rect().top();
                        let mut current = 0;
                        for (i, (title, about, keys)) in SECTIONS.iter().enumerate() {
                            let h = heading(ui, title, about, i > 0);
                            if state.go_to == Some(i) {
                                h.scroll_to_me(Some(Align::TOP));
                            }
                            // Lit on the left once its heading reaches the top.
                            if h.rect.top() - top <= viewport.top() + 40.0 {
                                current = i;
                            }
                            if i == 0
                                && let Some(c) = themes_row(ui, state, z, cfg, themes)
                            {
                                changed = Some(c);
                            }
                            if !keys.is_empty() {
                                card(ui, |ui| {
                                    for (n, key) in keys.iter().enumerate() {
                                        if n > 0 {
                                            divider(ui);
                                        }
                                        if let Some(c) = row(ui, state, z, cfg, key) {
                                            changed = Some(c);
                                        }
                                    }
                                });
                            }
                        }
                        ui.add_space(14.0);
                        ui.label(
                            RichText::new(format!("Saved to {}", z.dirs().config_file().display()))
                                .small()
                                .color(ui.visuals().weak_text_color()),
                        );
                        state.current = current;
                    });
                });
        });
    state.go_to = None;
    changed
}

/// A section's title and what it holds.
fn heading(ui: &mut Ui, title: &str, about: &str, gap: bool) -> egui::Response {
    if gap {
        ui.add_space(26.0);
    }
    let r = ui.label(RichText::new(title).font(crate::fonts::strong(ui.ctx(), 15.5)));
    ui.label(RichText::new(about).color(ui.visuals().weak_text_color()));
    ui.add_space(10.0);
    r
}

/// The rounded card a section's rows sit in.
fn card(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let v = ui.visuals();
    let fill = crate::chrome::palette(ui).map_or(v.panel_fill, |p| p.window_bg);
    egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, v.widgets.noninteractive.bg_stroke.color))
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(16, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

fn divider(ui: &mut Ui) {
    let line = ui
        .visuals()
        .widgets
        .noninteractive
        .bg_stroke
        .color
        .gamma_multiply(0.7);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, line);
}

/// One setting: its name and what it does on the left, its control on the
/// right, a refusal under it in red.
fn row(
    ui: &mut Ui,
    state: &mut SettingsState,
    z: &Zrush,
    cfg: &Config,
    key: &str,
) -> Option<Changed> {
    let (_, kind) = SETTINGS.iter().find(|(k, _)| *k == key)?;
    let mut result = None;
    ui.add_space(10.0);
    let control = control_width(*kind);
    let text_width = (ui.available_width() - control - 16.0).max(160.0);
    ui.horizontal_top(|ui| {
        let mut label = None;
        ui.allocate_ui_with_layout(vec2(text_width, 0.0), Layout::top_down(Align::Min), |ui| {
            // A switch's name flips it too, as on any settings screen.
            let sense = if *kind == Kind::Flag {
                Sense::click()
            } else {
                Sense::hover()
            };
            label = Some(
                ui.add(
                    egui::Label::new(label_for(key))
                        .sense(sense)
                        .selectable(false),
                )
                .on_hover_text(format!("{key} in config.toml")),
            );
            ui.add(
                egui::Label::new(
                    RichText::new(says(key))
                        .small()
                        .color(ui.visuals().weak_text_color()),
                )
                .wrap(),
            );
            if let Some(e) = state.errors.get(key) {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("{}: {e}", label_for(key)),
                );
            }
        });
        let flip = label.as_ref().is_some_and(egui::Response::clicked);
        let label = label.map(|l| l.id);
        let room = ui.available_width();
        ui.allocate_ui_with_layout(vec2(room, 0.0), Layout::right_to_left(Align::Min), |ui| {
            result = control_ui(ui, state, z, cfg, key, *kind, label);
        });
        if flip && result.is_none() {
            result = Some(z.edit_config(|c| {
                let _ = c.toggle(key);
            }));
        }
    });
    ui.add_space(10.0);
    match result? {
        Ok(cfg) => {
            state.errors.remove(key);
            state.drafts.remove(key);
            Some(Changed::Config(cfg))
        }
        Err(e) => {
            state.errors.insert(key.to_string(), e.to_string());
            None
        }
    }
}

/// Room the control of a kind of setting takes on the right.
fn control_width(kind: Kind) -> f32 {
    match kind {
        Kind::Flag => 40.0,
        Kind::Number => 80.0,
        Kind::Editor | Kind::Agent => 220.0,
        Kind::Words | Kind::Path => 300.0,
    }
}

type Outcome = zrush_core::error::Result<Config>;

fn control_ui(
    ui: &mut Ui,
    state: &mut SettingsState,
    z: &Zrush,
    cfg: &Config,
    key: &str,
    kind: Kind,
    label: Option<egui::Id>,
) -> Option<Outcome> {
    match kind {
        Kind::Flag => {
            let mut on = cfg.show(key) == "on";
            let mut r = toggle(ui, &mut on);
            if let Some(l) = label {
                r = r.labelled_by(l);
            }
            r.changed().then(|| {
                z.edit_config(|c| {
                    let _ = c.toggle(key);
                })
            })
        }
        Kind::Editor => {
            let current = KNOWN_EDITORS
                .iter()
                .position(|n| cfg.editor_cmd.is_empty() && cfg.editor == *n);
            segmented(ui, KNOWN_EDITORS, current)
                .map(|i| z.set_editor(KNOWN_EDITORS[i]).map(|()| z.config()))
        }
        Kind::Agent => {
            let ids: Vec<&str> = z.agents().iter().map(|a| a.id()).collect();
            let current = ids.iter().position(|id| cfg.agent.as_deref() == Some(*id));
            segmented(ui, &ids, current).map(|i| actions::switch_agent(z, ids[i]))
        }
        Kind::Number | Kind::Words | Kind::Path => field(ui, state, z, cfg, key, kind, label),
    }
}

/// A text field, saved when it loses focus; a path has a picker beside it.
fn field(
    ui: &mut Ui,
    state: &mut SettingsState,
    z: &Zrush,
    cfg: &Config,
    key: &str,
    kind: Kind,
    label: Option<egui::Id>,
) -> Option<Outcome> {
    let mut picked = None;
    if kind == Kind::Path && ui.button("Browse…").clicked() {
        picked = rfd::FileDialog::new().pick_folder();
    }
    let error = state.errors.contains_key(key);
    let draft = state
        .drafts
        .entry(key.to_string())
        .or_insert_with(|| cfg.show(key));
    if let Some(p) = &picked {
        *draft = p.display().to_string();
    }
    let hint = match key {
        "editor_cmd" => "code --reuse-window",
        "terminal" => "open -a Ghostty",
        "default_repo" => "~/projects/thing",
        _ => "",
    };
    let width = control_width(kind) - if kind == Kind::Path { 80.0 } else { 0.0 };
    let mut edit = egui::TextEdit::singleline(draft)
        .hint_text(hint)
        .desired_width(width)
        .margin(egui::Margin::symmetric(8, 5));
    if kind == Kind::Number {
        edit = edit.horizontal_align(Align::RIGHT);
    }
    if error {
        edit = edit.text_color(ui.visuals().error_fg_color);
    }
    let mut r = ui.add(edit);
    if let Some(l) = label {
        r = r.labelled_by(l);
    }
    if !(r.lost_focus() || picked.is_some()) {
        return None;
    }
    let value = draft.clone();
    if value == cfg.show(key) {
        return None;
    }
    let mut refused = None;
    let saved = z.edit_config(|c| refused = c.set(key, &value).err());
    Some(refused.map_or(saved, Err))
}

/// A switch, as a phone draws one: the accent when on.
fn toggle(ui: &mut Ui, on: &mut bool) -> egui::Response {
    let (rect, mut r) = ui.allocate_exact_size(vec2(36.0, 20.0), Sense::click());
    if r.clicked() {
        *on = !*on;
        r.mark_changed();
    }
    let value = *on;
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, value, ""));
    let t = ui.ctx().animate_bool_responsive(r.id, value);
    let (accent, _) = crate::chrome::accent(ui);
    let off = ui.visuals().widgets.noninteractive.bg_stroke.color;
    let track = lerp(off, accent, t);
    let p = ui.painter();
    p.rect_filled(rect, 10.0, track);
    let x = egui::lerp(rect.left() + 10.0..=rect.right() - 10.0, t);
    let knob = egui::pos2(x, rect.center().y);
    p.circle_filled(knob + vec2(0.0, 0.8), 8.2, Color32::from_black_alpha(40));
    p.circle_filled(knob, 8.0, Color32::WHITE);
    r
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        m(a.r(), b.r()),
        m(a.g(), b.g()),
        m(a.b(), b.b()),
        m(a.a(), b.a()),
    )
}

/// One of a few, side by side in a rounded track; the index picked.
fn segmented(ui: &mut Ui, options: &[&str], current: Option<usize>) -> Option<usize> {
    let mut picked = None;
    let v = ui.visuals();
    let track = v.extreme_bg_color;
    {
        egui::Frame::new()
            .fill(track)
            .corner_radius(8)
            .inner_margin(egui::Margin::same(2))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    // In a row laid from the right, the first comes last.
                    let order: Vec<usize> = if ui.layout().prefer_right_to_left() {
                        (0..options.len()).rev().collect()
                    } else {
                        (0..options.len()).collect()
                    };
                    for i in order {
                        let name: &str = options[i];
                        let on = current == Some(i);
                        let fill = if on {
                            crate::chrome::palette(ui)
                                .map_or(ui.visuals().panel_fill, |p| p.panel_bg)
                        } else {
                            Color32::TRANSPARENT
                        };
                        let b = egui::Button::new(RichText::new(name).size(13.0))
                            .fill(fill)
                            .stroke(if on {
                                Stroke::new(
                                    1.0,
                                    ui.visuals().widgets.noninteractive.bg_stroke.color,
                                )
                            } else {
                                Stroke::NONE
                            })
                            .corner_radius(6)
                            .min_size(vec2(56.0, 24.0))
                            .selected(on);
                        if ui.add(b).clicked() {
                            picked = Some(i);
                        }
                    }
                });
            });
    }
    picked
}

/// The themes as small pictures of the window, the one in use ringed.
fn themes_row(
    ui: &mut Ui,
    state: &mut SettingsState,
    z: &Zrush,
    cfg: &Config,
    themes: &[Theme],
) -> Option<Changed> {
    let mut picked: Option<String> = None;
    let dark = themes.iter().find(|t| t.dark).map(|t| t.palette);
    let light = themes.iter().find(|t| !t.dark).map(|t| t.palette);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(14.0, 14.0);
        if let (Some(d), Some(l)) = (dark, light)
            && tile(ui, "Auto", &[d, l], cfg.gui_theme == "auto")
                .on_hover_text("Follows the system's light or dark mode")
                .clicked()
        {
            picked = Some("auto".into());
        }
        for t in themes {
            if tile(ui, &t.label, &[t.palette], cfg.gui_theme == t.name).clicked() {
                picked = Some(t.name.clone());
            }
        }
    });
    if let Some(e) = state.errors.get("gui_theme") {
        ui.colored_label(ui.visuals().error_fg_color, e);
    }
    ui.add_space(4.0);
    let name = picked?;
    match z.edit_config(|c| c.gui_theme = name) {
        Ok(cfg) => {
            state.errors.remove("gui_theme");
            Some(Changed::Config(cfg))
        }
        Err(e) => {
            state.errors.insert("gui_theme".into(), e.to_string());
            None
        }
    }
}

/// A theme drawn in small: its sidebar, a few lines of text, a running
/// dot and its accent; two halves for one that follows the system.
fn tile(ui: &mut Ui, label: &str, palettes: &[Palette], on: bool) -> egui::Response {
    // One allocation of a known size, picture and name: a wrapping row
    // can only wrap what it can measure before placing it.
    let (whole, r) = ui.allocate_exact_size(vec2(128.0, 106.0), Sense::click());
    let rect = egui::Rect::from_min_size(whole.min, vec2(128.0, 80.0));
    // One palette, or two halves: a handful at most.
    let n = f32::from(u8::try_from(palettes.len()).unwrap_or(1));
    for (i, p) in (0u8..).zip(palettes) {
        let x0 = egui::lerp(rect.left()..=rect.right(), f32::from(i) / n);
        let x1 = egui::lerp(rect.left()..=rect.right(), f32::from(i + 1) / n);
        let part = egui::Rect::from_x_y_ranges(x0..=x1, rect.y_range());
        picture(&ui.painter().with_clip_rect(part), rect, p);
    }
    let (accent, _) = crate::chrome::accent(ui);
    let ring = if on {
        Stroke::new(2.0, accent)
    } else if r.hovered() {
        Stroke::new(1.0, ui.visuals().text_color().gamma_multiply(0.5))
    } else {
        Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color)
    };
    ui.painter()
        .rect_stroke(rect, 10.0, ring, egui::StrokeKind::Outside);
    // Painted, not a label: the tile says its name to a screen reader.
    let font = if on {
        crate::fonts::strong(ui.ctx(), 12.5)
    } else {
        egui::FontId::proportional(12.5)
    };
    ui.painter().text(
        whole.center_bottom(),
        egui::Align2::CENTER_BOTTOM,
        label,
        font,
        ui.visuals().text_color(),
    );
    r.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, label)
    });
    r
}

/// The window, as `p` colours it, in `rect`.
fn picture(painter: &egui::Painter, rect: egui::Rect, p: &Palette) {
    painter.rect_filled(rect, 10.0, p.window_bg);
    let side = egui::Rect::from_min_size(rect.min, vec2(38.0, rect.height()));
    painter.rect_filled(
        side,
        egui::CornerRadius {
            nw: 10,
            sw: 10,
            ne: 0,
            se: 0,
        },
        p.panel_bg,
    );
    let bar = |x: f32, y: f32, w: f32, c: Color32| {
        painter.rect_filled(
            egui::Rect::from_min_size(rect.min + vec2(x, y), vec2(w, 4.0)),
            2.0,
            c,
        );
    };
    // The tree: a running session, and two others.
    painter.circle_filled(rect.min + vec2(9.0, 16.0), 2.5, p.live);
    bar(14.0, 14.0, 18.0, p.text.gamma_multiply(0.8));
    bar(8.0, 26.0, 24.0, p.muted.gamma_multiply(0.7));
    bar(8.0, 36.0, 20.0, p.muted.gamma_multiply(0.7));
    // The page: a title, a few lines, the accent's button.
    bar(48.0, 14.0, 44.0, p.text);
    bar(48.0, 28.0, 66.0, p.muted.gamma_multiply(0.8));
    bar(48.0, 38.0, 56.0, p.muted.gamma_multiply(0.8));
    bar(48.0, 48.0, 62.0, p.muted.gamma_multiply(0.8));
    painter.rect_filled(
        egui::Rect::from_min_size(rect.min + vec2(92.0, 62.0), vec2(26.0, 9.0)),
        3.0,
        p.accent,
    );
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use zrush_core::config::{Config, dirs_under};
    use zrush_core::host::RecordingHost;

    use crate::app::GuiApp;
    use crate::testing::{git, scratch_repo};

    fn two_tabs(td: &tempfile::TempDir) -> GuiApp {
        let first = scratch_repo(td);
        let second = zrush_core::paths::real(td.path()).join("second");
        std::fs::create_dir_all(&second).unwrap();
        git(&second, &["init", "-q"]);
        git(&second, &["config", "user.email", "t@example.invalid"]);
        git(&second, &["config", "user.name", "zrush tests"]);
        std::fs::write(second.join("g.txt"), "x").unwrap();
        git(&second, &["add", "g.txt"]);
        git(&second, &["commit", "-qm", "init"]);
        let mut app = GuiApp::new(
            dirs_under(td.path()),
            Config::default(),
            vec![first, second],
            Box::new(|_| Box::new(RecordingHost::default())),
            Arc::new(|| {}),
        );
        app.open_settings();
        app
    }

    fn harness(app: GuiApp) -> Harness<'static, GuiApp> {
        let mut h = Harness::builder()
            .with_size([1200.0, 900.0])
            .build_eframe(|_cc| app);
        h.run_steps(3);
        h
    }

    fn saved(td: &tempfile::TempDir) -> Option<Config> {
        Config::load_file(&dirs_under(td.path())).ok()
    }

    #[test]
    fn a_flag_toggled_here_is_saved_and_every_tab_sees_it() {
        let td = tempfile::TempDir::new().unwrap();
        let mut h = harness(two_tabs(&td));
        h.get_by_label("Session titles").click_accesskit();
        h.run_steps(3);
        assert!(!saved(&td).unwrap().titles);
        assert_eq!(h.state().tab_count(), 2);
        for t in h.state().tabs() {
            assert!(!t.zrush().unwrap().config().titles, "{}", t.title);
        }
    }

    #[test]
    fn a_refused_number_shows_the_error_and_saves_nothing() {
        let td = tempfile::TempDir::new().unwrap();
        let before = saved(&td).map(|c| c.resumable_max);
        let mut h = harness(two_tabs(&td));
        let field = h.get_by_label("Sessions per worktree");
        field.focus();
        field.type_text("lots");
        h.key_press(egui::Key::Enter);
        h.run_steps(3);
        assert!(
            h.query_by_label_contains("Sessions per worktree:")
                .is_some()
        );
        assert_eq!(saved(&td).map(|c| c.resumable_max), before);
    }

    #[test]
    fn settings_show_what_is_saved_not_the_windows_own_override() {
        // The window reads whole conversations by overriding preview_turns
        // for itself; the settings must show, and keep, the saved value.
        let td = tempfile::TempDir::new().unwrap();
        let h = harness(two_tabs(&td));
        let field = h.get_by_label("Preview turns");
        assert_eq!(field.value().as_deref(), Some("14"));
    }

    #[test]
    fn picking_a_theme_saves_it() {
        let td = tempfile::TempDir::new().unwrap();
        let mut h = harness(two_tabs(&td));
        h.get_by_label("Nord").click();
        h.run_steps(2);
        assert_eq!(saved(&td).unwrap().gui_theme, "nord");
    }

    #[test]
    fn picking_an_editor_saves_it() {
        let td = tempfile::TempDir::new().unwrap();
        let mut h = harness(two_tabs(&td));
        h.get_by_label("code").click();
        h.run_steps(2);
        assert_eq!(saved(&td).unwrap().editor, "code");
    }
}
