//! The row list.
//!
//! Columns are measured in display width, not bytes: worktree labels and
//! session titles routinely carry accents, and the tree and status glyphs
//! are not one byte each either.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use unicode_width::UnicodeWidthStr;
use zrush_core::model::tree::{Row, RowKind};

use super::theme;

/// What the caller has decided about each row, kept out of `Row` so the
/// core stays free of interface state.
pub struct View<'a> {
    pub rows: &'a [Row],
    /// Shown as `~` in the path column. Display only: the rows keep the
    /// absolute path, which is what `--list` hands to a script.
    pub home: Option<&'a std::path::Path>,
    pub cursor: usize,
    /// Rows marked with space, by index. Purge mode only.
    pub marked: &'a std::collections::HashSet<usize>,
    /// Scroll offset, so a long list does not need the whole terminal.
    pub offset: usize,
}

fn style_for(kind: RowKind) -> Style {
    match kind {
        RowKind::Worktree => theme::worktree(),
        RowKind::Session | RowKind::Orphan => theme::session(),
        RowKind::Orphans => theme::orphans(),
        RowKind::More => theme::more(),
    }
}

/// Each column is as wide as its widest value, so nothing is truncated that
/// does not have to be.
fn widths(rows: &[Row]) -> (usize, usize, usize) {
    let mut label = 0;
    let mut badge = 0;
    let mut status = 0;
    for r in rows {
        label = label.max(
            UnicodeWidthStr::width(r.glyph.as_str()) + UnicodeWidthStr::width(r.label.as_str()),
        );
        badge = badge.max(UnicodeWidthStr::width(r.badge.as_str()));
        status = status.max(UnicodeWidthStr::width(r.status.as_str()));
    }
    (label, badge, status)
}

fn shorten(path: &str, home: Option<&std::path::Path>) -> String {
    let Some(home) = home.map(|h| h.to_string_lossy().into_owned()) else {
        return path.to_string();
    };
    match path.strip_prefix(&home) {
        Some(rest) => format!("~{rest}"),
        None => path.to_string(),
    }
}

/// Keep the tail. A path's last components say which worktree it is; its
/// first say which machine, which you already know.
fn elide_left(s: &str, to: usize) -> String {
    if to == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(s) <= to {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars().rev() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if w + cw > to.saturating_sub(1) {
            break;
        }
        out.push(c);
        w += cw;
    }
    let tail: String = out.chars().rev().collect();
    format!("…{tail}")
}

fn pad(s: &str, to: usize) -> String {
    let w = UnicodeWidthStr::width(s);
    let mut out = s.to_string();
    out.push_str(&" ".repeat(to.saturating_sub(w)));
    out
}

/// One row as coloured spans.
///
/// The selected row is drawn in ONE style rather than by reversing each
/// span: reversing turns every column's own colour into a different
/// background, which reads as a row of coloured blocks instead of a cursor.
pub fn line<'a>(
    row: &Row,
    w: (usize, usize, usize),
    marked: bool,
    selected: bool,
    room: usize,
    home: Option<&std::path::Path>,
) -> Line<'a> {
    let (lw, bw, sw) = w;
    // Whatever is left after the fixed columns and the three gaps.
    let for_path = room.saturating_sub(1 + lw + 2 + bw + 2 + sw + 2);
    let mark = if marked { "▌" } else { " " };
    let parts = [
        (mark.to_string(), theme::marked()),
        (
            pad(&format!("{}{}", row.glyph, row.label), lw),
            style_for(row.kind),
        ),
        ("  ".to_string(), Style::default()),
        (pad(&row.badge, bw), theme::badge()),
        ("  ".to_string(), Style::default()),
        (pad(&row.status, sw), theme::git()),
        ("  ".to_string(), Style::default()),
        (
            elide_left(&shorten(&row.location, home), for_path),
            theme::path(),
        ),
    ];
    let spans: Vec<Span<'a>> = parts
        .into_iter()
        .map(|(text, style)| Span::styled(text, if selected { theme::cursor() } else { style }))
        .collect();
    Line::from(spans)
}

pub fn render(f: &mut Frame, area: Rect, view: &View<'_>, title: &str) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::border())
        .title(Span::styled(format!(" {title} "), theme::title()));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let w = widths(view.rows);
    let height = inner.height as usize;
    let lines: Vec<Line> = view
        .rows
        .iter()
        .enumerate()
        .skip(view.offset)
        .take(height)
        .map(|(i, r)| {
            line(
                r,
                w,
                view.marked.contains(&i),
                i == view.cursor,
                inner.width as usize,
                view.home,
            )
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

/// Keep the cursor on screen: the offset moves only when it has to.
pub fn scroll_to(offset: usize, cursor: usize, height: usize) -> usize {
    if height == 0 {
        return 0;
    }
    if cursor < offset {
        cursor
    } else if cursor >= offset + height {
        cursor + 1 - height
    } else {
        offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zrush_core::model::tree::NodeId;

    fn row(kind: RowKind, glyph: &str, label: &str, badge: &str, status: &str) -> Row {
        Row {
            kind,
            node: NodeId::Orphans,
            session_id: None,
            glyph: glyph.into(),
            label: label.into(),
            badge: badge.into(),
            status: status.into(),
            location: String::new(),
        }
    }

    #[test]
    fn columns_are_as_wide_as_their_widest_value() {
        let rows = vec![
            row(RowKind::Worktree, "▾ ", "main", "● 1 live", "~3"),
            row(RowKind::Worktree, "▾ ", "a-much-longer-branch", "◌ 2", ""),
        ];
        let (label, badge, status) = widths(&rows);
        assert_eq!(label, 2 + "a-much-longer-branch".len());
        assert_eq!(badge, "● 1 live".chars().count());
        assert_eq!(status, 2);
    }

    #[test]
    fn padding_counts_display_width_not_bytes() {
        // é is two bytes and one column; ▾ is three bytes and one column.
        assert_eq!(UnicodeWidthStr::width(pad("é", 3).as_str()), 3);
        assert_eq!(UnicodeWidthStr::width(pad("▾ x", 5).as_str()), 5);
    }

    #[test]
    fn a_marked_row_carries_the_bar() {
        let r = row(RowKind::Session, "", "t", "", "");
        let marked = line(&r, (1, 0, 0), true, false, 80, None);
        assert!(marked.spans[0].content.contains('▌'));
        let plain = line(&r, (1, 0, 0), false, false, 80, None);
        assert_eq!(plain.spans[0].content.as_ref(), " ");
    }

    #[test]
    fn the_selected_row_is_one_colour_from_end_to_end() {
        // Reversing each span instead turns every column into a block of a
        // different colour.
        let r = row(RowKind::Worktree, "▾ ", "main", "● 1 live", "~3");
        let l = line(&r, (10, 10, 4), false, true, 80, None);
        let first = l.spans[0].style;
        assert!(
            l.spans.iter().all(|s| s.style == first),
            "the cursor row is not uniform"
        );
        assert!(first.bg.is_some());
    }

    #[test]
    fn an_unselected_row_keeps_its_own_colours() {
        let r = row(RowKind::Worktree, "▾ ", "main", "● 1 live", "~3");
        let l = line(&r, (10, 10, 4), false, false, 80, None);
        let styles: std::collections::HashSet<_> = l.spans.iter().map(|s| s.style).collect();
        assert!(
            styles.len() > 1,
            "every column the same colour is not a table"
        );
    }

    #[test]
    fn a_path_too_long_for_the_pane_keeps_its_tail() {
        assert_eq!(elide_left("/a/b/c/worktrees/rust", 10), "…rees/rust");
        assert_eq!(elide_left("/short", 10), "/short");
        assert_eq!(elide_left("/a/b", 0), "");
    }

    #[test]
    fn an_elided_path_never_exceeds_its_room() {
        for room in 1..30 {
            let out = elide_left("/private/var/folders/x/y/repo/.claude/worktrees/rust", room);
            assert!(
                UnicodeWidthStr::width(out.as_str()) <= room,
                "{room}: {out}"
            );
        }
    }

    #[test]
    fn a_row_never_draws_past_the_pane() {
        let r = row(RowKind::Worktree, "▾ ", "main [root]", "● 1 live", "~3");
        let mut r = r;
        r.location = "/private/var/folders/3c/6v5l9jgd/T/zrush/repo/.claude/worktrees/rust".into();
        let l = line(&r, (13, 8, 2), false, false, 50, None);
        let drawn: usize = l
            .spans
            .iter()
            .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
            .sum();
        assert!(drawn <= 50, "drew {drawn} columns into 50");
    }

    #[test]
    fn the_home_directory_becomes_a_tilde() {
        let home = std::path::Path::new("/Users/you");
        assert_eq!(
            shorten("/Users/you/projects/wt", Some(home)),
            "~/projects/wt"
        );
        assert_eq!(shorten("/opt/thing", Some(home)), "/opt/thing");
        assert_eq!(
            shorten("/Users/you/projects/wt", None),
            "/Users/you/projects/wt"
        );
    }

    #[test]
    fn the_offset_follows_the_cursor_down() {
        assert_eq!(scroll_to(0, 5, 10), 0, "still on screen");
        assert_eq!(scroll_to(0, 12, 10), 3, "just below the fold");
    }

    #[test]
    fn the_offset_follows_the_cursor_up() {
        assert_eq!(scroll_to(5, 2, 10), 2);
    }

    #[test]
    fn a_zero_height_pane_does_not_divide_by_it() {
        assert_eq!(scroll_to(4, 9, 0), 0);
    }
}
