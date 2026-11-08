//! Putting the pieces on the terminal, and the loop that keeps them there.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::mpsc;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use zrush_core::actions;
use zrush_core::app::Zrush;
use zrush_core::error::ZrushError;

use super::app::{Action, App, Mode};
use super::preview::Preview;
use super::{header, modal, preview, table, theme};

mod events;
mod run;
#[cfg(test)]
mod test_support;

pub use events::{apply, preview_for, rebuild, want_preview};
pub use run::run;

/// How wide the preview pane is. Below this the terminal is too narrow for
/// two panes and the list takes it all.
const PREVIEW_PERCENT: u16 = 50;
const MIN_WIDTH_FOR_PREVIEW: u16 = 100;

pub fn draw(f: &mut Frame, app: &App, z: &Zrush, prev: &Preview) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header::HEIGHT),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(f.area());

    let branch = app
        .snap
        .worktrees
        .iter()
        .find(|w| w.is_main)
        .map_or("", |w| w.branch.as_str());
    let repo = z
        .main_root()
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    header::render(
        f,
        rows[0],
        &header::Info {
            repo: &repo,
            branch,
            agent: if app.active_agent.is_empty() {
                "none"
            } else {
                &app.active_agent
            },
            worktrees: app.snap.worktrees.len(),
            live: app.snap.live.len(),
            resumable: app.snap.resumable.len(),
            loaded: app.snap.scanned,
            update: app.update.as_deref(),
        },
    );

    let body = if rows[1].width >= MIN_WIDTH_FOR_PREVIEW {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(100 - PREVIEW_PERCENT),
                Constraint::Percentage(PREVIEW_PERCENT),
            ])
            .split(rows[1])
    } else {
        Layout::default()
            .constraints([Constraint::Percentage(100)])
            .split(rows[1])
    };

    let visible: Vec<zrush_core::model::tree::Row> = app
        .visible
        .iter()
        .filter_map(|&i| app.rows.get(i).cloned())
        .collect();
    let marked: HashSet<usize> = app
        .visible
        .iter()
        .enumerate()
        .filter(|(_, i)| app.marked.contains(i))
        .map(|(shown, _)| shown)
        .collect();
    let title = match app.mode {
        Mode::Purge => format!("Purge — {} marked", app.marked.len()),
        _ => format!("Worktrees({})", app.snap.worktrees.len()),
    };
    table::render(
        f,
        body[0],
        &table::View {
            rows: &visible,
            matched: &app.matched,
            home: app.home.as_deref(),
            cursor: app.cursor,
            marked: &marked,
            offset: app.offset,
        },
        &title,
    );
    if body.len() > 1 {
        preview::render(f, body[1], prev);
    }

    status_line(f, rows[2], app);

    if let Some(m) = &app.modal {
        modal::render(f, f.area(), m);
    }
}

/// One frame per tick. Braille rather than ASCII: it turns in place
/// instead of jumping a column, and every terminal that draws the tree's
/// box characters has it.
const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

fn status_line(f: &mut Frame, area: Rect, app: &App) {
    let left = match app.mode {
        Mode::Filter => format!(
            "/{}▏   fuzzy · \x27sub · =exact · ^start · end$ · !not",
            app.filter
        ),
        Mode::Purge => "space marks · enter purges · esc cancels".into(),
        Mode::Normal if !app.filter.is_empty() => format!("/{}", app.filter),
        Mode::Normal => "<esc> quit".into(),
    };
    // What is happening now outranks what happened last: a spinner that
    // the previous action's flash sits on top of is worse than no spinner.
    let right = match &app.busy {
        Some(what) => {
            let frame = SPINNER[app.spin % SPINNER.len()];
            format!("{frame} {what}…")
        }
        None => app.flash.clone().unwrap_or_default(),
    };
    let gap =
        (area.width as usize).saturating_sub(left.chars().count() + right.chars().count() + 2);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {left}"), theme::header_label()),
            Span::raw(" ".repeat(gap)),
            Span::styled(
                right,
                if app.busy.is_some() {
                    theme::header_key()
                } else {
                    theme::badge()
                },
            ),
        ])),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::test_support::*;

    #[test]
    fn the_frame_shows_the_worktrees_and_their_sessions() {
        let td = tempfile::TempDir::new().unwrap();
        let (z, app) = loaded(&td);
        let text = frame(&app, &z, 120, 20);
        assert!(text.contains("main [root]"));
        assert!(text.contains("rust"));
        assert!(text.contains("Conversion en Rust"));
        assert!(text.contains("Publier zrush sur GitHub"));
    }

    #[test]
    fn the_header_names_the_agent_and_counts_what_is_loaded() {
        let td = tempfile::TempDir::new().unwrap();
        let (z, app) = loaded(&td);
        let text = frame(&app, &z, 120, 20);
        assert!(text.contains("Agent"));
        assert!(text.contains("claude"));
        assert!(text.contains("1 live"));
        assert!(text.contains("1 resumable"));
    }

    #[test]
    fn the_title_counts_the_worktrees() {
        let td = tempfile::TempDir::new().unwrap();
        let (z, app) = loaded(&td);
        assert!(frame(&app, &z, 120, 20).contains("Worktrees(2)"));
    }

    #[test]
    fn a_narrow_terminal_drops_the_preview_rather_than_squashing_it() {
        let td = tempfile::TempDir::new().unwrap();
        let (z, app) = loaded(&td);
        assert!(frame(&app, &z, 120, 20).contains("Preview"));
        assert!(!frame(&app, &z, 80, 20).contains("Preview"));
    }

    #[test]
    fn a_modal_covers_the_list_rather_than_sitting_beside_it() {
        let td = tempfile::TempDir::new().unwrap();
        let (z, mut app) = loaded(&td);
        app.modal = Some(crate::ui::modal::Modal::Help);
        let text = frame(&app, &z, 120, 24);
        assert!(text.contains("Keys"));
        assert!(text.contains("ctrl-p"));
    }

    #[test]
    fn purge_mode_says_so_in_the_title_and_the_status_line() {
        let td = tempfile::TempDir::new().unwrap();
        let (z, mut app) = loaded(&td);
        app.mode = crate::ui::app::Mode::Purge;
        app.marked.insert(0);
        let text = frame(&app, &z, 120, 20);
        assert!(text.contains("Purge — 1 marked"));
        assert!(text.contains("space marks"));
    }

    #[test]
    fn the_filter_shows_what_is_being_typed() {
        let td = tempfile::TempDir::new().unwrap();
        let (z, mut app) = loaded(&td);
        app.mode = crate::ui::app::Mode::Filter;
        app.filter = "rust".into();
        assert!(frame(&app, &z, 120, 20).contains("/rust"));
    }

    #[test]
    fn a_flash_message_reaches_the_status_line() {
        let td = tempfile::TempDir::new().unwrap();
        let (z, mut app) = loaded(&td);
        app.flash("created feature");
        assert!(frame(&app, &z, 120, 20).contains("created feature"));
    }

    #[test]
    fn a_tiny_terminal_draws_without_panicking() {
        let td = tempfile::TempDir::new().unwrap();
        let (z, app) = loaded(&td);
        for (w, h) in [(20_u16, 6_u16), (40, 10), (200, 60)] {
            let _ = frame(&app, &z, w, h);
        }
    }
}
