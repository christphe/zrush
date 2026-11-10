//! The right pane: the conversation of the selected session, or the git
//! state of the selected worktree.

use std::borrow::Cow;

use egui::{Align, Frame, Layout, Margin, RichText};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use zrush_core::agent::Role;
use zrush_core::intent::Question;
use zrush_core::loader::Preview;

use crate::tab::Tab;

/// How tall a line of markdown is. The window's 26 px rows suit buttons;
/// `egui_commonmark` lays its list markers and links at the top of a row, so
/// in a row that tall they float above the text beside them.
const MD_ROW: f32 = 18.0;

pub fn show(ui: &mut egui::Ui, tab: &mut Tab, md: &mut CommonMarkCache) -> Option<Question> {
    let Some(row) = tab.selected_row().cloned() else {
        empty(ui);
        return None;
    };

    let palette = crate::theme::palette(ui.ctx());
    let muted = ui.visuals().weak_text_color();
    let mut asked = None;
    ui.horizontal(|ui| {
        // A worktree's branch, or a session's title, and under it where it
        // stands and where it lives.
        ui.vertical(|ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(row.search.as_str()).font(crate::fonts::strong(ui.ctx(), 19.0)),
                )
                .truncate(),
            );
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                state(ui, tab, &row);
                let home = std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map(std::path::PathBuf::from);
                let path = zrush_core::paths::shorten(&row.location, home.as_deref());
                if !path.is_empty() {
                    ui.label(RichText::new(path).small().color(muted));
                }
            });
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add(egui::Button::new("›_  Terminal").min_size(egui::vec2(0.0, 28.0)))
                .on_hover_text("The agent in a terminal of its own")
                .clicked()
            {
                tab.run_agent(&row);
            }
            if crate::chrome::primary_button(ui, "Open in IDE").clicked() {
                asked = tab.enter(&row);
            }
        });
    });
    ui.add_space(10.0);
    let line = palette.map_or(ui.visuals().widgets.noninteractive.bg_stroke.color, |p| {
        p.border
    });
    let (r, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().rect_filled(r, 0.0, line);

    // A new selection starts at its end: the last thing said is what tells
    // you where the session got to.
    let scroll_id = egui::Id::new(("conversation", tab.selected.as_deref().unwrap_or("")));
    egui::ScrollArea::vertical()
        .id_salt(scroll_id)
        .auto_shrink(false)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            // No wider than the pane: a wide code block would otherwise
            // widen everything, and the text would run off the right.
            ui.set_max_width(ui.available_width());
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
                    // Measured once, before any turn is drawn. A code block
                    // is drawn a few pixels wider than it is given, and
                    // egui widens the parent to fit: measured per turn,
                    // every turn after it would wrap a little further right.
                    let width = ui.available_width();
                    for (i, t) in turns.iter().enumerate() {
                        // Close together within one speaker's run, apart
                        // where the other one takes over.
                        let same = i > 0 && turns[i - 1].role == t.role;
                        let more = turns.get(i + 1).is_some_and(|n| n.role == t.role);
                        ui.add_space(if same { SAME_RUN } else { NEW_RUN });
                        in_column(ui, i, width, |ui| {
                            turn(ui, t, !same, more, md, palette);
                        });
                    }
                    ui.add_space(16.0);
                }
            }
        });
    asked
}

/// Draw one turn in a column `width` wide at the cursor, in a child the
/// column does not grow with: a paragraph egui measures a few hundred
/// points too far left (a long path wrapping after inline code in a list
/// item) would shift every turn after it. The column takes the turn's
/// height at its own width, and cuts whatever spills out.
fn in_column(ui: &mut egui::Ui, i: usize, width: f32, add: impl FnOnce(&mut egui::Ui)) {
    let top = ui.cursor().min;
    let column = egui::Rect::from_min_size(top, egui::vec2(width, f32::INFINITY));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(i)
            .max_rect(column)
            .layout(Layout::top_down(Align::Min)),
    );
    child.set_clip_rect(ui.clip_rect().intersect(column));
    add(&mut child);
    let height = child.min_rect().bottom() - top.y;
    ui.advance_cursor_after_rect(egui::Rect::from_min_size(top, egui::vec2(width, height)));
}

/// Room on the left of an agent's turn, where its dot sits.
const GUTTER: f32 = 22.0;

/// What you said sits in a box as wide as the page; what the agent
/// answered reads as the page itself, a dot in the margin beside each
/// turn and a thread joining the turns of one run. Each says when, on the
/// right, when the transcript knows.
fn turn(
    ui: &mut egui::Ui,
    t: &zrush_core::agent::Turn,
    first: bool,
    more: bool,
    md: &mut CommonMarkCache,
    palette: Option<crate::theme::Palette>,
) {
    let muted = ui.visuals().weak_text_color();
    if first && let Some(when) = t.at.and_then(|at| zrush_core::model::clock_here(now(), at)) {
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            ui.label(RichText::new(when).small().color(muted));
        });
        ui.add_space(2.0);
    }
    match t.role {
        Role::User => {
            let v = ui.visuals();
            Frame::new()
                .fill(v.faint_bg_color)
                .stroke(egui::Stroke::new(
                    1.0,
                    v.widgets.noninteractive.bg_stroke.color,
                ))
                .corner_radius(8)
                .inner_margin(Margin::symmetric(14, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    markdown(ui, md, &t.text);
                });
        }
        Role::Assistant => {
            let colour = palette.map_or(muted, |p| p.agent_turn);
            let top = ui.cursor().min;
            // The letters sit in the middle of their line.
            let row = reading_row(ui);
            let r = ui.horizontal_top(|ui| {
                ui.add_space(GUTTER);
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    markdown(ui, md, &t.text);
                });
            });
            let dot = egui::pos2(top.x + 6.0, top.y + row / 2.0);
            let p = ui.painter();
            p.circle_filled(dot, 3.5, colour);
            // The thread down to the next turn of the run, through the
            // room between them.
            if more {
                let line = colour.gamma_multiply(0.35);
                p.line_segment(
                    [
                        dot + egui::vec2(0.0, 8.0),
                        egui::pos2(dot.x, r.response.rect.bottom() + SAME_RUN + row / 2.0 - 8.0),
                    ],
                    egui::Stroke::new(1.0, line),
                );
            }
        }
    }
}

/// Room between two turns of one speaker, and where the other takes over.
const SAME_RUN: f32 = 14.0;
const NEW_RUN: f32 = 26.0;

/// Seconds since the epoch, as the transcript's times are kept.
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// How tall one line of running text is.
fn reading_row(ui: &egui::Ui) -> f32 {
    let font = crate::fonts::reading(ui.ctx(), READING_SIZE);
    ui.fonts_mut(|f| f.row_height(&font))
}

use crate::fonts::READING_SIZE;

/// Under the title: running, or how long ago, for a session; nothing more
/// for a worktree, whose state is the pane itself.
fn state(ui: &mut egui::Ui, tab: &Tab, row: &zrush_core::model::tree::Row) {
    let Some(s) = row
        .session_id
        .as_deref()
        .and_then(|id| tab.snap.session(id))
    else {
        return;
    };
    let a = crate::status::Activity::of(s);
    let colour = crate::status::colour(ui, a);
    match s.kind {
        zrush_core::model::SessionKind::Live => {
            crate::chrome::dot_chip(ui, a, &format!("running · {}", s.status), colour, "");
        }
        zrush_core::model::SessionKind::Resumable => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
            let age = zrush_core::model::age(now, s.last_activity);
            crate::chrome::dot_chip(ui, a, &format!("resumable · {age} ago"), colour, "");
        }
    }
}

/// An HTML comment `egui_commonmark` hands back to us, where a line must end.
const BREAK: &str = "<!--zrush-break-->";

fn markdown(ui: &mut egui::Ui, md: &mut CommonMarkCache, text: &str) {
    let html = |ui: &mut egui::Ui, html: &str| {
        if html.trim() == BREAK {
            ui.end_row();
        } else {
            // What egui_commonmark draws for HTML when nobody renders it.
            ui.label(html);
        }
    };
    ui.scope(|ui| {
        // Read in the face with room between its lines; a row of widgets
        // as tall as one of its lines, so a list's marker, a link or a
        // word in code sits on the line it belongs to.
        let font = crate::fonts::reading(ui.ctx(), READING_SIZE);
        let row = ui.fonts_mut(|f| f.row_height(&font));
        ui.style_mut()
            .text_styles
            .insert(egui::TextStyle::Body, font);
        let code = crate::fonts::mono_reading(ui.ctx());
        ui.style_mut()
            .text_styles
            .insert(egui::TextStyle::Monospace, code);
        ui.spacing_mut().interact_size.y = row.max(MD_ROW);
        CommonMarkViewer::new().render_html_fn(Some(&html)).show(
            ui,
            md,
            &break_before_fences(text),
        );
    });
}

/// A line break ahead of every code fence that follows text.
///
/// `egui_commonmark` starts a code block wherever the cursor is: in a list
/// item written without blank lines, that is the end of the item's text,
/// and the block lands beside it instead of under it. An HTML comment can
/// interrupt the text where a blank line would not help (a list item's
/// paragraph does not end its line either), and `markdown` turns it into
/// the end of the row. A fence already open is left alone, so nothing
/// inside a code block changes.
fn break_before_fences(text: &str) -> Cow<'_, str> {
    let mut out = String::new();
    let mut open: Option<(char, usize)> = None;
    let mut prev_blank = true;
    let mut changed = false;
    for line in text.split_inclusive('\n') {
        let fence = fence(line);
        match (open, fence) {
            (None, Some(f)) => {
                if !prev_blank {
                    let indent = &line[..line.len() - line.trim_start().len()];
                    out.push_str(indent);
                    out.push_str(BREAK);
                    out.push('\n');
                    changed = true;
                }
                open = Some(f);
            }
            (Some((c, n)), Some((fc, fn_)))
                if fc == c
                    && fn_ >= n
                    && line.trim_start().trim_start_matches(c).trim().is_empty() =>
            {
                open = None;
            }
            _ => {}
        }
        prev_blank = line.trim().is_empty();
        out.push_str(line);
    }
    if changed {
        Cow::Owned(out)
    } else {
        Cow::Borrowed(text)
    }
}

/// The fence character and its length, when this line opens or closes a
/// code block.
fn fence(line: &str) -> Option<(char, usize)> {
    let t = line.trim_start();
    let c = t.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = t.chars().take_while(|x| *x == c).count();
    (n >= 3).then_some((c, n))
}

/// Nothing selected yet: say what this pane is for.
fn empty(ui: &mut egui::Ui) {
    let muted = ui.visuals().weak_text_color();
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.30);
        crate::chrome::mark(ui, 44.0);
        ui.add_space(14.0);
        ui.label(RichText::new("Pick a session").font(crate::fonts::strong(ui.ctx(), 18.0)));
        ui.add_space(4.0);
        ui.label(
            RichText::new("Its conversation shows here. A worktree shows its git state.")
                .color(muted),
        );
        ui.add_space(18.0);
        // A grid as wide as its content, in the middle: the gestures in a
        // column, what they do lined up beside them.
        let width = 280.0;
        ui.horizontal(|ui| {
            ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
            egui::Grid::new("gestures")
                .num_columns(2)
                .spacing([12.0, 10.0])
                .show(ui, |ui| {
                    for (keys, what) in [
                        ("double click".to_string(), "open in the editor"),
                        ("right click".to_string(), "every action on a row"),
                        (crate::chrome::shortcut("F"), "filter the tree"),
                        (crate::chrome::shortcut("K"), "find anything, do anything"),
                    ] {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            crate::chrome::chip(ui, &keys, muted, "");
                        });
                        ui.label(RichText::new(what).color(muted));
                        ui.end_row();
                    }
                });
        });
    });
}

#[cfg(test)]
mod tests {
    use egui_commonmark::CommonMarkCache;
    use egui_kittest::kittest::Queryable;

    use super::{BREAK, break_before_fences};

    #[test]
    fn a_fence_right_after_a_list_items_text_gets_a_break_at_its_indent() {
        let text = "1. after each build:\n   ```sh\n   codesign\n   ```\n";
        assert_eq!(
            break_before_fences(text),
            format!("1. after each build:\n   {BREAK}\n   ```sh\n   codesign\n   ```\n")
        );
    }

    #[test]
    fn a_fence_after_a_blank_line_is_left_alone() {
        let text = "text\n\n```\ncode\n```\n";
        assert!(matches!(
            break_before_fences(text),
            std::borrow::Cow::Borrowed(_)
        ));
    }

    #[test]
    fn the_empty_pane_names_the_shortcuts_as_the_platform_does() {
        let mut h = egui_kittest::Harness::builder()
            .with_size([800.0, 600.0])
            .build_ui(super::empty);
        h.run();
        for key in ["F", "K"] {
            let label = crate::chrome::shortcut(key);
            assert!(h.query_by_label(&label).is_some(), "no {label}");
        }
        if !cfg!(target_os = "macos") {
            assert!(h.query_by_label_contains("⌘").is_none());
        }
    }

    #[test]
    fn nothing_inside_a_code_block_changes() {
        // A longer fence holds a shorter one as plain text, and a closing
        // fence is never mistaken for an opening one.
        let text = "````md\nsay\n```\nhi\n```\n````\nafter\n";
        assert_eq!(break_before_fences(text), text);
    }

    #[test]
    fn tilde_fences_count_too() {
        let text = "- run:\n  ~~~\n  x\n  ~~~\n";
        assert!(break_before_fences(text).contains(BREAK));
    }

    #[test]
    fn the_break_is_never_drawn_as_text() {
        let mut md = CommonMarkCache::default();
        let h = egui_kittest::Harness::new_ui(move |ui| {
            super::markdown(
                ui,
                &mut md,
                "- after each build:\n  ```\n  codesign\n  ```\n",
            );
        });
        assert!(h.query_by_label_contains("zrush-break").is_none());
        assert!(h.query_by_label_contains("after each build").is_some());
    }

    /// How far the turn after `add`'s starts from where `add`'s did.
    fn drift(add: impl FnOnce(&mut egui::Ui) + 'static) -> f32 {
        let shift = std::rc::Rc::new(std::cell::Cell::new(0.0));
        let out = shift.clone();
        let mut add = Some(add);
        let _h = egui_kittest::Harness::new_ui(move |ui| {
            let Some(add) = add.take() else { return };
            let before = ui.cursor().left();
            super::in_column(ui, 0, 500.0, add);
            super::in_column(ui, 1, 500.0, |ui| {
                out.set(ui.max_rect().left() - before);
            });
        });
        shift.get()
    }

    #[test]
    fn a_turn_drawn_too_far_left_moves_nothing_else() {
        let moved = drift(|ui| {
            let at = ui.cursor().min - egui::vec2(300.0, 0.0);
            ui.allocate_rect(
                egui::Rect::from_min_size(at, egui::vec2(10.0, 10.0)),
                egui::Sense::hover(),
            );
        });
        assert!(moved.abs() < 0.5, "the column moved {moved}");
    }

    #[test]
    fn a_long_path_after_inline_code_in_a_list_moves_nothing() {
        // The line that once pushed a whole conversation 328 points left.
        let line = "   - Every change was verified with captures. The final `./check` \
                    passes in full. Contact sheet at /private/tmp/claude-502/-Users-christphe-\
                    projects-lafourche-wt/d686a47d-f3b1-4bbb-a876-61036edbe66a/scratchpad/x.png";
        let moved = drift(move |ui| {
            let mut md = CommonMarkCache::default();
            super::markdown(ui, &mut md, line);
        });
        assert!(moved.abs() < 0.5, "the column moved {moved}");
    }
}
