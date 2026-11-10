//! ⌘K: everything zrush can do or show, found by typing a few letters of
//! it. The app's commands, each tab's, every worktree and session of every
//! open repository, ranked by the filter's own matcher.

use std::path::PathBuf;

use egui::{Align, Key, Layout, RichText, Sense, vec2};
use zrush_core::model::tree::{Row, RowKind};

use crate::menu::Command;

/// What an entry does once picked.
#[derive(Clone, Debug)]
pub enum Target {
    App(Command),
    /// Bring this repository's tab to the front.
    Tab(PathBuf),
    /// Show this row, in its repository's tab.
    Row(PathBuf, Box<Row>),
    /// Something done in one tab.
    InTab(PathBuf, TabAction),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabAction {
    NewWorktree,
    Purge,
    Refresh,
}

#[derive(Clone, Debug)]
pub struct Entry {
    /// What is matched against, and shown.
    pub title: String,
    /// Where it lives, said quietly beside it.
    pub detail: String,
    /// The kind of thing it is: a word on the right.
    pub kind: &'static str,
    pub target: Target,
}

#[derive(Default)]
pub struct State {
    pub open: bool,
    query: String,
    /// The highlighted line, among the ranked ones.
    cursor: usize,
}

impl State {
    pub fn toggle(&mut self) {
        if self.open {
            self.open = false;
        } else {
            *self = Self {
                open: true,
                ..Self::default()
            };
        }
    }
}

/// The entries for the tabs open now: the app's commands, then for each
/// tab its own commands, its worktrees and its sessions.
pub fn entries<'a>(tabs: impl Iterator<Item = &'a crate::tab::Tab>) -> Vec<Entry> {
    let mut out = vec![
        app("Open Repository…", Command::OpenRepository),
        app("Settings", Command::Settings),
    ];
    let tabs: Vec<_> = tabs.collect();
    for t in &tabs {
        let repo = &t.repo;
        if tabs.len() > 1 {
            out.push(Entry {
                title: format!("Go to {}", t.title),
                detail: String::new(),
                kind: "Tab",
                target: Target::Tab(repo.clone()),
            });
        }
        for (title, action) in [
            ("New worktree", TabAction::NewWorktree),
            ("Purge worktrees and sessions…", TabAction::Purge),
            ("Refresh", TabAction::Refresh),
        ] {
            out.push(Entry {
                title: title.into(),
                detail: t.title.clone(),
                kind: "Command",
                target: Target::InTab(repo.clone(), action),
            });
        }
    }
    for t in &tabs {
        let mut worktree = String::new();
        for r in t.rows() {
            let kind = match r.kind {
                RowKind::Worktree => {
                    worktree.clone_from(&r.search);
                    "Worktree"
                }
                RowKind::Session | RowKind::Orphan => "Session",
                RowKind::Orphans => {
                    worktree = "orphans".into();
                    continue;
                }
                RowKind::More => continue,
            };
            let detail = if r.kind == RowKind::Worktree {
                t.title.clone()
            } else {
                format!("{} · {worktree}", t.title)
            };
            out.push(Entry {
                title: r.search.clone(),
                detail,
                kind,
                target: Target::Row(t.repo.clone(), Box::new(r.clone())),
            });
        }
    }
    out
}

fn app(title: &str, c: Command) -> Entry {
    Entry {
        title: title.into(),
        detail: String::new(),
        kind: "Command",
        target: Target::App(c),
    }
}

/// How many ranked lines are drawn: past that, typing narrows faster than
/// scrolling would.
const SHOWN: usize = 40;
const WIDTH: f32 = 600.0;
const LINE: f32 = 34.0;

/// Draw the palette over the window when it is open; what was picked, if
/// anything.
pub fn show(ctx: &egui::Context, state: &mut State, entries: &[Entry]) -> Option<Target> {
    if !state.open {
        return None;
    }
    let titles: Vec<&str> = entries.iter().map(|e| e.title.as_str()).collect();
    let ranked = zrush_core::filter::rank(&titles, &state.query);
    let count = ranked.len().min(SHOWN);
    let (moved, enter) = keys(ctx, state, count)?;
    let mut picked = (enter && count > 0).then_some(state.cursor);

    let screen = ctx.content_rect();
    // The window behind, dimmed; a click there closes.
    let backdrop = egui::Area::new(egui::Id::new("palette-backdrop"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            ui.painter().with_clip_rect(screen).rect_filled(
                screen,
                0.0,
                egui::Color32::from_black_alpha(90),
            );
            ui.interact(screen, ui.id().with("dim"), Sense::click())
                .clicked()
        })
        .inner;
    let width = WIDTH.min(screen.width() - 32.0);
    egui::Area::new(egui::Id::new("palette"))
        .order(egui::Order::Tooltip)
        .fixed_pos(egui::pos2(
            screen.center().x - width / 2.0,
            screen.top() + 90.0,
        ))
        .show(ctx, |ui| {
            let v = ui.visuals().clone();
            egui::Frame::new()
                .fill(v.window_fill)
                .stroke(v.window_stroke)
                .corner_radius(12)
                .shadow(v.popup_shadow)
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.set_width(width - 16.0);
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut state.query)
                            .hint_text("Type a command, a worktree, a session…")
                            .font(egui::FontId::proportional(16.0))
                            .frame(egui::Frame::NONE)
                            .margin(egui::Margin::symmetric(8, 8))
                            .desired_width(f32::INFINITY),
                    );
                    field.request_focus();
                    if field.changed() {
                        state.cursor = 0;
                    }
                    let line = v.widgets.noninteractive.bg_stroke.color;
                    let (r, _) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                    ui.painter().rect_filled(r, 0.0, line);
                    ui.add_space(6.0);
                    if count == 0 {
                        ui.add_space(6.0);
                        ui.weak("  Nothing matches");
                        ui.add_space(8.0);
                        return;
                    }
                    egui::ScrollArea::vertical()
                        .max_height((screen.height() - 260.0).clamp(LINE * 3.0, LINE * 11.0))
                        .show(ui, |ui| {
                            for (n, (i, matched)) in ranked.iter().take(SHOWN).enumerate() {
                                let e = &entries[*i];
                                let on = n == state.cursor;
                                let r = line_ui(ui, e, matched, on);
                                if on && moved {
                                    r.scroll_to_me(None);
                                }
                                if r.clicked() {
                                    picked = Some(n);
                                }
                                if r.hovered()
                                    && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO)
                                {
                                    state.cursor = n;
                                }
                            }
                        });
                });
        });
    if backdrop && picked.is_none() {
        state.open = false;
        return None;
    }
    let n = picked?;
    state.open = false;
    ranked
        .get(n)
        .and_then(|(i, _)| entries.get(*i))
        .map(|e| e.target.clone())
}

/// The keys the palette answers: arrows move, Escape closes (`None`).
/// Whether the cursor moved, and whether Enter was pressed.
fn keys(ctx: &egui::Context, state: &mut State, count: usize) -> Option<(bool, bool)> {
    let (up, down, enter, escape) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
            i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
            i.consume_key(egui::Modifiers::NONE, Key::Enter),
            i.consume_key(egui::Modifiers::NONE, Key::Escape),
        )
    });
    if escape {
        state.open = false;
        return None;
    }
    if down && count > 0 {
        state.cursor = (state.cursor + 1) % count;
    }
    if up && count > 0 {
        state.cursor = (state.cursor + count - 1) % count;
    }
    state.cursor = state.cursor.min(count.saturating_sub(1));
    Some((up || down, enter))
}

/// One entry: its title with what matched lit, where it lives, its kind.
fn line_ui(ui: &mut egui::Ui, e: &Entry, matched: &[usize], on: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), LINE), Sense::click());
    let v = ui.visuals();
    if on {
        ui.painter().rect_filled(rect, 8.0, v.selection.bg_fill);
    }
    let muted = v.weak_text_color();
    let font = egui::TextStyle::Body.resolve(ui.style());
    let title = crate::tree_view::highlighted(ui, &e.title, matched, font);
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(10.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(e.kind).small().color(muted));
                ui.add_space(8.0);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add(egui::Label::new(title).selectable(false));
                    if !e.detail.is_empty() {
                        ui.add_space(6.0);
                        ui.add(
                            egui::Label::new(RichText::new(&e.detail).color(muted))
                                .truncate()
                                .selectable(false),
                        );
                    }
                });
            });
        },
    );
    response
}
