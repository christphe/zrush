//! The right-hand pane.
//!
//! A worktree gets its git status and recent commits with who wrote them; a
//! session gets the tail of the conversation, which is the thing you
//! actually want to see before resuming one.
//!
//! A conversation is anchored to its end, not its start: the last thing
//! said is what tells you where the session got to. That means wrapping the
//! text here rather than leaving it to `Paragraph`, which can only count
//! its rows after it has drawn them.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use zrush_core::agent::{Role, Turn};

use super::theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preview {
    /// Nothing to show, or nothing fetched yet.
    Empty,
    /// Lines of `git status` and `git log`, already fetched.
    Worktree(Vec<String>),
    Conversation(Vec<Turn>),
}

pub fn render(f: &mut Frame, area: Rect, preview: &Preview) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::border())
        .title(Span::styled(" Preview ", theme::title()));
    // One column of air, so the text does not touch the border the way the
    // table's rows do not.
    let full = block.inner(area);
    let inner = ratatui::layout::Rect {
        x: full.x + 1,
        width: full.width.saturating_sub(2),
        ..full
    };
    f.render_widget(block, area);

    match preview {
        Preview::Empty => {}
        Preview::Worktree(rows) => {
            let lines: Vec<Line> = rows
                .iter()
                .map(|r| Line::from(Span::styled(r.clone(), theme::dim())))
                .collect();
            f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
        }
        Preview::Conversation(turns) => {
            let mut lines = conversation(turns, inner.width as usize);
            // Keep the end: a conversation that overflows the pane loses
            // its opening, never its last word.
            let over = lines.len().saturating_sub(inner.height as usize);
            lines.drain(..over);
            f.render_widget(Paragraph::new(lines), inner);
        }
    }
}

/// The width of `you ` and `  · `, so a wrapped turn stays under its text.
const TAG: usize = 4;

fn conversation(turns: &[Turn], width: usize) -> Vec<Line<'static>> {
    let mut out: Vec<Line> = Vec::new();
    for t in turns {
        let (tag, style) = match t.role {
            Role::User => ("you", theme::worktree()),
            Role::Assistant => ("  ·", theme::dim()),
        };
        let body = width.saturating_sub(TAG).max(1);
        for (i, row) in wrap(&t.text, body).into_iter().enumerate() {
            let head = if i == 0 {
                Span::styled(format!("{tag} "), style)
            } else {
                Span::raw(" ".repeat(TAG))
            };
            out.push(Line::from(vec![head, Span::raw(row)]));
        }
        out.push(Line::from(Span::raw("")));
    }
    out
}

/// Word wrap, breaking a word too long for a line of its own. The text
/// arrives with its whitespace already squashed, so splitting on spaces is
/// the whole job.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut rows: Vec<String> = Vec::new();
    let mut row = String::new();
    for word in text.split(' ') {
        let mut word = word;
        if !row.is_empty() && row.chars().count() + 1 + word.chars().count() > width {
            rows.push(std::mem::take(&mut row));
        }
        while word.chars().count() > width {
            let cut = word
                .char_indices()
                .nth(width)
                .map_or(word.len(), |(i, _)| i);
            rows.push(word[..cut].to_string());
            word = &word[cut..];
        }
        if !row.is_empty() {
            row.push(' ');
        }
        row.push_str(word);
    }
    if !row.is_empty() || rows.is_empty() {
        rows.push(row);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw(p: &Preview) -> String {
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 14)).unwrap();
        term.draw(|f| render(f, f.area(), p)).unwrap();
        super::super::tests_support::flatten(term.backend())
    }

    #[test]
    fn a_conversation_marks_who_said_what() {
        let turns = vec![
            Turn {
                role: Role::User,
                text: "port it to rust".into(),
            },
            Turn {
                role: Role::Assistant,
                text: "on it".into(),
            },
        ];
        let text = draw(&Preview::Conversation(turns));
        assert!(text.contains("you"));
        assert!(text.contains("port it to rust"));
        assert!(text.contains("on it"));
    }

    #[test]
    fn a_worktree_shows_the_lines_it_was_given() {
        let text = draw(&Preview::Worktree(vec![
            "## main...origin/main".into(),
            "M f.txt".into(),
        ]));
        assert!(text.contains("origin/main"));
        assert!(text.contains("f.txt"));
    }

    /// The point of the pane: a session you are about to resume is judged
    /// on where it got to, not on how it opened.
    #[test]
    fn a_long_conversation_shows_its_end_and_drops_its_start() {
        let turns: Vec<Turn> = (0..40)
            .map(|i| Turn {
                role: if i % 2 == 0 {
                    Role::User
                } else {
                    Role::Assistant
                },
                text: format!("turn number {i}"),
            })
            .collect();
        let text = draw(&Preview::Conversation(turns));
        assert!(text.contains("turn number 39"), "the last turn is missing");
        assert!(!text.contains("turn number 0 "), "the first turn survived");
    }

    #[test]
    fn a_wrapped_turn_sits_under_its_own_text() {
        let lines = conversation(
            &[Turn {
                role: Role::User,
                text: "one two three four five".into(),
            }],
            14,
        );
        // "you one two", then two indented rows under it.
        assert!(lines.len() >= 3);
        assert_eq!(lines[1].spans[0].content.as_ref(), "    ");
    }

    #[test]
    fn a_word_too_long_for_the_line_is_broken_rather_than_lost() {
        assert_eq!(
            wrap("aaaaaa", 4),
            vec!["aaaa".to_string(), "aa".to_string()]
        );
        assert_eq!(wrap("ab cd", 4), vec!["ab".to_string(), "cd".to_string()]);
        assert_eq!(wrap("", 4), vec![String::new()]);
    }

    #[test]
    fn an_empty_preview_still_draws_its_frame() {
        let text = draw(&Preview::Empty);
        assert!(text.contains("Preview"));
    }

    #[test]
    fn a_long_line_wraps_instead_of_being_cut() {
        let long = "x".repeat(200);
        let text = draw(&Preview::Conversation(vec![Turn {
            role: Role::User,
            text: long,
        }]));
        // More than one row of x's means it wrapped.
        assert!(text.matches("xxxxx").count() > 1);
    }
}
