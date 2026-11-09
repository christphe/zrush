//! The left pane: worktrees as foldable headers, their sessions under them.

use egui::{Align, Layout, RichText, Sense, UiBuilder, vec2};
use zrush_core::intent::Question;
use zrush_core::model::SessionKind;
use zrush_core::model::tree::{Row, RowKind};
use zrush_core::snapshot::preview_key;

use crate::status;
use crate::tab::Tab;

/// Height of one session row: room to aim for, and to breathe.
const ROW: f32 = 26.0;

/// What a click in the tree asks for, done after drawing so the rows are
/// not borrowed while the tab changes.
enum Act {
    Select(Row),
    Enter(Row),
}

pub fn show(ui: &mut egui::Ui, tab: &mut Tab) -> Option<Question> {
    let muted = ui.visuals().weak_text_color();
    ui.horizontal(|ui| {
        ui.label(RichText::new("WORKTREES").small().strong().color(muted));
        let n = tab.snap.worktrees.len();
        ui.label(
            RichText::new(if n == 1 { "1".into() } else { n.to_string() })
                .small()
                .color(muted),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .small_button("⟳")
                .on_hover_text("Look again now")
                .clicked()
            {
                tab.refresh();
            }
            if let Some(b) = &tab.busy {
                ui.label(RichText::new(b.as_str()).small().color(muted));
                ui.spinner();
            }
        });
    });
    ui.add_space(4.0);

    let mut acts: Vec<Act> = Vec::new();
    let selected = tab.selected.clone();
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            for (head, children) in groups(tab.rows()) {
                group(ui, tab, head, &children, selected.as_deref(), &mut acts);
                ui.add_space(6.0);
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
    let root = head.facts.root;
    let count = children.len();
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, root || live)
        .show_header(ui, |ui| header(ui, head, count, selected, acts))
        .body(|ui| {
            if children.is_empty() {
                ui.weak("No sessions yet");
            }
            for c in children {
                child(ui, tab, c, selected, acts);
            }
        });
}

/// A worktree's line: its name, what it holds, and the button that opens
/// it. The right-hand side is laid out first and the name takes what is
/// left, cut short with "…": a long branch never runs under the counts.
fn header(
    ui: &mut egui::Ui,
    head: &Row,
    children: usize,
    selected: Option<&str>,
    acts: &mut Vec<Act>,
) {
    let muted = ui.visuals().weak_text_color();
    let picked = selected.is_some() && preview_key(head).as_deref() == selected;
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if head.kind == RowKind::Worktree
            && ui
                .small_button("Open")
                .on_hover_text("Open this worktree in the editor, with a fresh conversation")
                .clicked()
        {
            acts.push(Act::Enter(head.clone()));
        }
        if !head.status.is_empty() {
            ui.label(
                RichText::new(head.status.as_str())
                    .small()
                    .monospace()
                    .color(muted),
            )
            .on_hover_text("git status: ~ changed, + staged, ? untracked, ↑↓ ahead/behind");
        }
        let badge = badge(head, children);
        if !badge.is_empty() {
            ui.label(RichText::new(badge).small().color(muted));
        }
        if head.facts.detached {
            pill(ui, "detached", "On a commit, not on a branch");
        }
        if head.facts.root {
            pill(ui, "root", "The main worktree, the one holding .git");
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            let name =
                egui::Button::selectable(picked, RichText::new(name(head)).strong()).truncate();
            let label = ui.add(name).on_hover_text(&head.location);
            if label.clicked() {
                acts.push(Act::Select(head.clone()));
            }
            if label.double_clicked() {
                acts.push(Act::Enter(head.clone()));
            }
        });
    });
}

/// What a worktree is called: its branch, or its folder when it is on a
/// commit and the branch says nothing.
fn name(head: &Row) -> String {
    if head.facts.detached
        && let Some(leaf) = std::path::Path::new(&head.location).file_name()
    {
        return leaf.to_string_lossy().into_owned();
    }
    head.search.clone()
}

/// A small outlined word beside a name.
fn pill(ui: &mut egui::Ui, word: &str, hover: &str) {
    let muted = ui.visuals().weak_text_color();
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, muted.gamma_multiply(0.6)))
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(5, 0))
        .show(ui, |ui| {
            ui.label(RichText::new(word).small().color(muted));
        })
        .response
        .on_hover_text(hover);
}

/// One session: a row as wide as the pane, its dot, its title, and how
/// long ago at the end.
fn child(ui: &mut egui::Ui, tab: &Tab, row: &Row, selected: Option<&str>, acts: &mut Vec<Act>) {
    if row.kind == RowKind::More {
        ui.indent(("more", &row.node), |ui| {
            if ui.link(row.label.trim()).clicked() {
                acts.push(Act::Enter(row.clone()));
            }
        });
        return;
    }
    let picked = selected.is_some() && preview_key(row).as_deref() == selected;
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
    let v = ui.visuals();
    if picked {
        ui.painter().rect_filled(rect, 6.0, v.selection.bg_fill);
    } else if response.hovered() {
        ui.painter().rect_filled(
            rect,
            6.0,
            v.widgets.hovered.weak_bg_fill.gamma_multiply(0.6),
        );
    }
    let kind = kind_of(tab, row);
    let (when, when_colour) = when(tab, row, ui);
    let misfiled = row.misfiled;
    let warn = v.warn_fg_color;
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect.shrink2(vec2(8.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            if let Some(k) = kind {
                status::dot(ui, k);
            }
            ui.add_space(2.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(when).small().color(when_colour));
                if misfiled {
                    ui.label(RichText::new("filed elsewhere").small().color(warn))
                        .on_hover_text(
                            "The agent filed this conversation under another directory: \
                             opening it here would start a fresh one.",
                        );
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(row.search.as_str())
                            .truncate()
                            .selectable(false)
                            .sense(Sense::hover()),
                    );
                });
            });
        },
    );
    if response.clicked() {
        acts.push(Act::Select(row.clone()));
    }
    if response.double_clicked() {
        acts.push(Act::Enter(row.clone()));
    }
}

/// The end of a session's row: how long ago for one that can resume,
/// what it is doing for one that runs. Read from the session itself, not
/// from the terminal's text for it.
fn when(tab: &Tab, row: &Row, ui: &egui::Ui) -> (String, egui::Color32) {
    let muted = ui.visuals().weak_text_color();
    let Some(s) = row
        .session_id
        .as_deref()
        .and_then(|id| tab.snap.session(id))
    else {
        return (String::new(), muted);
    };
    match s.kind {
        SessionKind::Live => {
            let colour =
                crate::theme::palette(ui.ctx()).map_or(ui.visuals().hyperlink_color, |p| p.live);
            (s.status.clone(), colour)
        }
        SessionKind::Resumable => (zrush_core::model::age(now(), s.last_activity), muted),
    }
}

/// Seconds since the epoch, as session times are kept.
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// What a worktree's badge counts, from its facts.
fn badge(head: &Row, children: usize) -> String {
    let f = head.facts;
    if f.live > 0 {
        format!("{} live", f.live)
    } else if f.history > 0 {
        format!("{} resumable", f.history)
    } else if head.kind == RowKind::Orphans && children > 0 {
        format!("{children} orphaned")
    } else {
        String::new()
    }
}

fn kind_of(tab: &Tab, row: &Row) -> Option<SessionKind> {
    let id = row.session_id.as_deref()?;
    tab.snap.session(id).map(|s| s.kind)
}
