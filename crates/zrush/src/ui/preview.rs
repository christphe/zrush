//! The right-hand pane.
//!
//! A worktree gets its git status and recent commits; a session gets the
//! tail of the conversation, which is the thing you actually want to see
//! before resuming one.

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
    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines: Vec<Line> = match preview {
        Preview::Empty => Vec::new(),
        Preview::Worktree(rows) => rows
            .iter()
            .map(|r| Line::from(Span::styled(r.clone(), theme::dim())))
            .collect(),
        Preview::Conversation(turns) => turns
            .iter()
            .flat_map(|t| {
                let (tag, style) = match t.role {
                    Role::User => ("you", theme::worktree()),
                    Role::Assistant => ("  ·", theme::dim()),
                };
                vec![
                    Line::from(vec![
                        Span::styled(format!("{tag} "), style),
                        Span::raw(t.text.clone()),
                    ]),
                    Line::from(Span::raw("")),
                ]
            })
            .collect(),
    };
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
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
