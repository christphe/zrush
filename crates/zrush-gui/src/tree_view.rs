//! The left pane: worktrees as foldable headers, their sessions under them.
//! Every row has its actions on a right click; the bar above the tree has
//! the ones that belong to no row.

use egui::{Align, Layout, RichText, Sense, UiBuilder, vec2};
use zrush_core::filter;
use zrush_core::intent::Question;
use zrush_core::model::SessionKind;
use zrush_core::model::tree::{NodeId, Row, RowKind};
use zrush_core::snapshot::preview_key;

use crate::status;
use crate::tab::Tab;

/// Height of one session row: room to aim for, and to breathe.
const ROW: f32 = 30.0;

/// What a click in the tree asks for, done after drawing so the rows are
/// not borrowed while the tab changes.
enum Act {
    Select(Row),
    Enter(Row),
    Terminal(Row),
    /// From a session's row the session moves to it; otherwise a fresh one.
    NewWorktree(Option<Row>),
    Delete(Row),
    Copy(String),
    Toggle(Row),
    Purge,
    AskPurge,
    LeavePurge,
}

/// What every row needs to know to draw itself.
struct Ctx<'a> {
    tab: &'a Tab,
    /// What the filter matched in each row's title, by row: positions in
    /// `Row::search`.
    marks: std::collections::HashMap<String, Vec<usize>>,
    selected: Option<&'a str>,
    purging: bool,
}

pub fn show(ui: &mut egui::Ui, tab: &mut Tab) -> Option<Question> {
    let muted = ui.visuals().weak_text_color();
    let mut acts: Vec<Act> = Vec::new();
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("WORKTREES")
                .font(crate::fonts::strong(ui.ctx(), 11.0))
                .color(muted),
        );
        let n = tab.snap.worktrees.len();
        ui.label(RichText::new(n.to_string()).small().color(muted));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if crate::chrome::icon_button(ui, "⟳", "Look again now").clicked() {
                tab.refresh();
            }
            if tab.purging.is_none() {
                if crate::chrome::icon_button(
                    ui,
                    "🗑",
                    "Purge: tick worktrees and sessions, then remove the lot",
                )
                .clicked()
                {
                    acts.push(Act::Purge);
                }
                if crate::chrome::icon_button(
                    ui,
                    "+",
                    "New worktree, on a new branch or an existing one",
                )
                .clicked()
                {
                    acts.push(Act::NewWorktree(None));
                }
            }
            if let Some(b) = &tab.busy {
                ui.spinner();
                ui.label(RichText::new(b.as_str()).small().color(muted));
            }
        });
    });
    ui.add_space(6.0);
    filter_field(ui, tab);
    ui.add_space(6.0);
    if tab.purging.is_some() {
        bar(ui, tab, &mut acts);
        ui.add_space(6.0);
    }

    let selected = tab.selected.clone();
    let filtering = !tab.filter.trim().is_empty();
    let (shown, marks) = filtered(tab);
    let cx = Ctx {
        tab,
        marks,
        selected: selected.as_deref(),
        purging: tab.purging.is_some(),
    };
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            if shown.is_empty() && filtering {
                ui.weak("No match");
            }
            for (head, children) in groups(shown.iter().copied()) {
                group(ui, &cx, head, &children, filtering, &mut acts);
                ui.add_space(6.0);
            }
        });

    let ctx = ui.ctx().clone();
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
            Act::Terminal(r) => tab.run_agent(&r),
            Act::NewWorktree(from) => tab.new_worktree(from.as_ref()),
            Act::Delete(r) => tab.ask_delete(&r),
            Act::Copy(text) => {
                ctx.copy_text(text.clone());
                tab.flash = Some(format!("copied {text}"));
            }
            Act::Toggle(r) => tab.toggle_mark(&r),
            Act::Purge => tab.start_purge(),
            Act::AskPurge => tab.ask_purge(),
            Act::LeavePurge => tab.leave_purge(),
        }
    }
    asked
}

/// The rows the filter keeps, in order, and what it matched in each.
fn filtered(tab: &Tab) -> (Vec<&Row>, std::collections::HashMap<String, Vec<usize>>) {
    let hits = filter::hits(tab.rows(), &tab.filter);
    let mut marks = std::collections::HashMap::new();
    let mut shown: Vec<&Row> = Vec::with_capacity(hits.len());
    for h in hits {
        let Some(row) = tab.rows().get(h.index) else {
            continue;
        };
        if !h.matched.is_empty()
            && let Some(key) = preview_key(row)
        {
            marks.insert(key, h.matched);
        }
        shown.push(row);
    }
    (shown, marks)
}

/// Purge mode's bar: what is ticked, and the way out.
fn bar(ui: &mut egui::Ui, tab: &Tab, acts: &mut Vec<Act>) {
    ui.horizontal(|ui| {
        if let Some(marked) = &tab.purging {
            let warn = ui.visuals().warn_fg_color;
            ui.label(RichText::new("Tick what goes").small().color(warn));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("Done").clicked() {
                    acts.push(Act::LeavePurge);
                }
                let go = egui::Button::new(
                    RichText::new(format!("Purge {}", marked.len()))
                        .color(ui.visuals().error_fg_color),
                )
                .small();
                if ui.add_enabled(!marked.is_empty(), go).clicked() {
                    acts.push(Act::AskPurge);
                }
            });
        }
    });
}

/// The field that narrows the tree. Ctrl+F (⌘F on macOS) reaches it from anywhere
/// in the tab; Escape empties it.
fn filter_field(ui: &mut egui::Ui, tab: &mut Tab) {
    let field = ui
        .add(
            egui::TextEdit::singleline(&mut tab.filter)
                .hint_text("🔍  Filter")
                .margin(egui::Margin::symmetric(8, 5))
                .desired_width(f32::INFINITY),
        )
        .on_hover_text(format!(
            "Fuzzy by default.  'exact  ^start  end$  !not\n{} to get here, Escape to clear.",
            crate::chrome::shortcut("F")
        ));
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F)) {
        field.request_focus();
    }
    let mut changed = field.changed();
    if field.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        tab.filter.clear();
        changed = true;
    }
    if changed {
        tab.filter_changed();
    }
}

/// The flat rows as headers (a worktree, or the orphans) and what is under
/// each.
fn groups<'a>(rows: impl IntoIterator<Item = &'a Row>) -> Vec<(&'a Row, Vec<&'a Row>)> {
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
    cx: &Ctx<'_>,
    head: &Row,
    children: &[&Row],
    filtering: bool,
    acts: &mut Vec<Act>,
) {
    let live = children
        .iter()
        .any(|c| kind_of(cx.tab, c) == Some(SessionKind::Live));
    // While filtering or purging every row shows, under a fold state of its
    // own: the folds you made come back afterwards. Ticking rows you cannot
    // see would be a trap.
    let forced = filtering || cx.purging;
    let id = ui.make_persistent_id(("node", forced, &head.node));
    let open = forced || head.facts.root || live;
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, open)
        .show_header(ui, |ui| header(ui, cx, head, children.len(), acts))
        .body(|ui| {
            if children.is_empty() {
                ui.weak("No sessions yet");
            }
            for c in children {
                child(ui, cx, c, acts);
            }
        });
}

/// A worktree's line: its name, what it holds, and the button that opens
/// it. The right-hand side is laid out first and the name takes what is
/// left, cut short with "…": a long branch never runs under the counts.
fn header(ui: &mut egui::Ui, cx: &Ctx<'_>, head: &Row, children: usize, acts: &mut Vec<Act>) {
    let muted = ui.visuals().weak_text_color();
    let picked = cx.selected.is_some() && preview_key(head).as_deref() == cx.selected;
    let tickable = cx.purging && head.kind == RowKind::Worktree && !head.facts.root;
    // "Open" only under the pointer: on every row at once it is noise.
    let hovered = ui.rect_contains_pointer(ui.max_rect());
    let live = crate::chrome::palette(ui).map_or(ui.visuals().hyperlink_color, |p| p.live);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        // Always laid out, shown only under the pointer: appearing and
        // vanishing would renumber the widgets after it, and a right click
        // on the name would lose its menu as the pointer moved.
        if head.kind == RowKind::Worktree
            && !cx.purging
            && ui
                .add_visible(hovered, egui::Button::new("Open").small())
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
        let f = head.facts;
        if f.live > 0 {
            crate::chrome::dot_chip(
                ui,
                status::Activity::Idle,
                &f.live.to_string(),
                live,
                "running now",
            );
        } else {
            let badge = badge(head, children);
            if !badge.is_empty() {
                ui.label(RichText::new(badge).small().color(muted));
            }
        }
        if head.facts.detached {
            crate::chrome::chip(ui, "detached", muted, "On a commit, not on a branch");
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            if tickable {
                tick(ui, cx.tab.is_marked(head), head, acts);
            }
            let shown = if tickable {
                cx.tab.is_marked(head)
            } else {
                picked
            };
            let font = crate::fonts::strong(ui.ctx(), 13.5);
            let shown_name = name(head);
            let matched = if shown_name == head.search {
                cx.matched(head)
            } else {
                &[]
            };
            let text = highlighted(ui, &shown_name, matched, font);
            let name = egui::Button::selectable(shown, text).truncate();
            let hover = if head.facts.root {
                format!("{}\nThe main worktree, the one holding .git", head.location)
            } else {
                head.location.clone()
            };
            let label = ui.add(name).on_hover_text(hover);
            if cx.purging {
                if tickable && label.clicked() {
                    acts.push(Act::Toggle(head.clone()));
                }
                return;
            }
            if label.clicked() {
                acts.push(Act::Select(head.clone()));
            }
            if label.double_clicked() {
                acts.push(Act::Enter(head.clone()));
            }
            if head.kind == RowKind::Worktree {
                label.context_menu(|ui| worktree_menu(ui, head, acts));
            }
        });
    });
}

impl Ctx<'_> {
    /// What the filter matched in `row`'s title.
    fn matched(&self, row: &Row) -> &[usize] {
        preview_key(row)
            .and_then(|k| self.marks.get(&k))
            .map_or(&[], Vec::as_slice)
    }
}

/// `text` with the characters at `matched` in the accent, underlined: what
/// the filter found, letter by letter. A name that is not `Row::search`
/// (a detached worktree's folder) is given no positions, so none are lit.
pub fn highlighted(
    ui: &egui::Ui,
    text: &str,
    matched: &[usize],
    font: egui::FontId,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let plain = egui::TextFormat {
        font_id: font,
        color: egui::Color32::PLACEHOLDER,
        valign: Align::Center,
        ..Default::default()
    };
    let (accent, _) = crate::chrome::accent(ui);
    let lit = egui::TextFormat {
        color: accent,
        underline: egui::Stroke::new(1.0, accent),
        ..plain.clone()
    };
    // Runs of lit and unlit characters, one section each.
    let mut run = String::new();
    let mut on = false;
    for (i, c) in text.chars().enumerate() {
        let now = matched.contains(&i);
        if now != on && !run.is_empty() {
            job.append(&run, 0.0, if on { lit.clone() } else { plain.clone() });
            run.clear();
        }
        on = now;
        run.push(c);
    }
    if !run.is_empty() {
        job.append(&run, 0.0, if on { lit } else { plain });
    }
    job
}

/// What a worktree's right click offers.
fn worktree_menu(ui: &mut egui::Ui, head: &Row, acts: &mut Vec<Act>) {
    item(ui, "Open in IDE", acts, || Act::Enter(head.clone()));
    item(ui, "Agent in a terminal", acts, || {
        Act::Terminal(head.clone())
    });
    item(ui, "New worktree", acts, || Act::NewWorktree(None));
    item(ui, "Copy path", acts, || Act::Copy(head.location.clone()));
    ui.separator();
    if head.facts.root {
        ui.add_enabled(false, egui::Button::new("Remove worktree"))
            .on_disabled_hover_text("The main worktree stays: git will not remove it");
    } else {
        item(ui, "Remove worktree", acts, || Act::Delete(head.clone()));
    }
}

/// What a session's right click offers. A running one cannot be deleted.
fn session_menu(ui: &mut egui::Ui, cx: &Ctx<'_>, row: &Row, acts: &mut Vec<Act>) {
    let session = row
        .session_id
        .as_deref()
        .and_then(|id| cx.tab.snap.session(id));
    item(ui, "Open in IDE", acts, || Act::Enter(row.clone()));
    item(ui, "Resume in a terminal", acts, || {
        Act::Terminal(row.clone())
    });
    item(ui, "Move to a new worktree", acts, || {
        Act::NewWorktree(Some(row.clone()))
    });
    let path = match (&row.node, session) {
        (NodeId::Worktree(p), _) => Some(p.display().to_string()),
        (NodeId::Orphans, Some(s)) => Some(s.cwd.display().to_string()),
        (NodeId::Orphans, None) => None,
    };
    if let Some(p) = path {
        item(ui, "Copy path", acts, || Act::Copy(p.clone()));
    }
    ui.separator();
    if session.is_some_and(|s| s.kind == SessionKind::Live) {
        ui.add_enabled(false, egui::Button::new("Delete conversation"))
            .on_disabled_hover_text("It is running: stop it first");
    } else {
        item(ui, "Delete conversation", acts, || Act::Delete(row.clone()));
    }
}

/// One menu entry: a click pushes what `act` makes and closes the menu.
fn item(ui: &mut egui::Ui, label: &str, acts: &mut Vec<Act>, act: impl FnOnce() -> Act) {
    if ui.button(label).clicked() {
        acts.push(act());
        ui.close();
    }
}

/// A purge-mode tick box.
fn tick(ui: &mut egui::Ui, marked: bool, row: &Row, acts: &mut Vec<Act>) {
    let mut on = marked;
    if ui.add(egui::Checkbox::without_text(&mut on)).changed() {
        acts.push(Act::Toggle(row.clone()));
    }
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

/// One session: a row as wide as the pane, its dot, its title, and how
/// long ago at the end. In purge mode a click ticks it instead.
fn child(ui: &mut egui::Ui, cx: &Ctx<'_>, row: &Row, acts: &mut Vec<Act>) {
    if row.kind == RowKind::More {
        ui.indent(("more", &row.node), |ui| {
            if ui.link(row.label.trim()).clicked() {
                acts.push(Act::Enter(row.clone()));
            }
        });
        return;
    }
    let kind = kind_of(cx.tab, row);
    let activity = activity_of(cx.tab, row);
    let tickable = cx.purging && kind != Some(SessionKind::Live);
    let marked = cx.tab.is_marked(row);
    let picked = if cx.purging {
        marked
    } else {
        cx.selected.is_some() && preview_key(row).as_deref() == cx.selected
    };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
    let v = ui.visuals();
    if picked {
        ui.painter().rect_filled(rect, 6.0, v.selection.bg_fill);
        // A bar of the accent down the left edge: the row being read.
        let (accent, _) = crate::chrome::accent(ui);
        let bar =
            egui::Rect::from_min_size(rect.min + vec2(0.0, 6.0), vec2(3.0, rect.height() - 12.0));
        ui.painter().rect_filled(bar, 2.0, accent);
    } else if response.hovered() {
        ui.painter().rect_filled(
            rect,
            6.0,
            v.widgets.hovered.weak_bg_fill.gamma_multiply(0.6),
        );
    }
    let (when, when_colour) = when(cx.tab, row, ui);
    let misfiled = row.misfiled;
    let warn = v.warn_fg_color;
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect.shrink2(vec2(8.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            if tickable {
                tick(ui, marked, row, acts);
            } else if let Some(a) = activity {
                status::dot(ui, a);
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
                    let font = egui::TextStyle::Body.resolve(ui.style());
                    ui.add(
                        egui::Label::new(highlighted(ui, &row.search, cx.matched(row), font))
                            .truncate()
                            .selectable(false)
                            .sense(Sense::hover()),
                    );
                });
            });
        },
    );
    if cx.purging {
        if tickable && response.clicked() {
            acts.push(Act::Toggle(row.clone()));
        }
        return;
    }
    if response.clicked() {
        acts.push(Act::Select(row.clone()));
    }
    if response.double_clicked() {
        acts.push(Act::Enter(row.clone()));
    }
    response.context_menu(|ui| session_menu(ui, cx, row, acts));
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
    match status::Activity::of(s) {
        a @ status::Activity::Busy => (s.status.clone(), status::colour(ui, a)),
        status::Activity::Idle => (s.status.clone(), muted),
        status::Activity::Resumable => (zrush_core::model::age(now(), s.last_activity), muted),
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

fn activity_of(tab: &Tab, row: &Row) -> Option<status::Activity> {
    let id = row.session_id.as_deref()?;
    tab.snap.session(id).map(status::Activity::of)
}

fn kind_of(tab: &Tab, row: &Row) -> Option<SessionKind> {
    let id = row.session_id.as_deref()?;
    tab.snap.session(id).map(|s| s.kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(job: &egui::text::LayoutJob) -> String {
        job.sections
            .iter()
            .filter(|s| s.format.underline.width > 0.0)
            .map(|s| &job.text[s.byte_range.start.0..s.byte_range.end.0])
            .collect()
    }

    #[test]
    fn the_letters_the_filter_found_are_lit() {
        let ctx = egui::Context::default();
        let mut job = None;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            job = Some(highlighted(
                ui,
                "réglages",
                &[0, 1, 7],
                egui::FontId::proportional(13.0),
            ));
        });
        out.textures_delta.clear();
        assert_eq!(lit(&job.unwrap()), "rés");
    }

    #[test]
    fn nothing_matched_is_plain() {
        let ctx = egui::Context::default();
        let mut job = None;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            job = Some(highlighted(
                ui,
                "plain",
                &[],
                egui::FontId::proportional(13.0),
            ));
        });
        out.textures_delta.clear();
        let job = job.unwrap();
        assert_eq!(lit(&job), "");
        assert_eq!(job.sections.len(), 1);
    }
}
