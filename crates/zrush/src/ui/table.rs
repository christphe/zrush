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

fn pad(s: &str, to: usize) -> String {
    let w = UnicodeWidthStr::width(s);
    let mut out = s.to_string();
    out.push_str(&" ".repeat(to.saturating_sub(w)));
    out
}

/// One row as coloured spans. Public so a snapshot test can read a line
/// without going through a terminal.
pub fn line<'a>(row: &Row, w: (usize, usize, usize), marked: bool) -> Line<'a> {
    let (lw, bw, sw) = w;
    let mark = if marked { "▌" } else { " " };
    let label = pad(&format!("{}{}", row.glyph, row.label), lw);
    Line::from(vec![
        Span::styled(mark.to_string(), theme::marked()),
        Span::styled(label, style_for(row.kind)),
        Span::raw("  "),
        Span::styled(pad(&row.badge, bw), theme::badge()),
        Span::raw("  "),
        Span::styled(pad(&row.status, sw), theme::git()),
        Span::raw("  "),
        Span::styled(row.location.clone(), theme::path()),
    ])
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
            let l = line(r, w, view.marked.contains(&i));
            if i == view.cursor {
                l.style(theme::cursor())
            } else {
                l
            }
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
        let marked = line(&r, (1, 0, 0), true);
        assert!(marked.spans[0].content.contains('▌'));
        let plain = line(&r, (1, 0, 0), false);
        assert_eq!(plain.spans[0].content.as_ref(), " ");
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
