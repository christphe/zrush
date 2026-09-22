//! Dialogs drawn over the list.
//!
//! This is what a native interface buys over driving fzf. Asking a question
//! used to mean rewriting the footer, rebinding `y`, `n` and `c`, unbinding
//! six other keys and restoring all of it afterwards. Here a modal is a
//! value in `App`, and drawing it is `Clear` plus a block.
//!
//! While one is up it owns the keyboard: there is no state in which a key
//! means two things.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};

use super::theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub label: String,
    /// Red, for the answer that destroys something.
    pub destructive: bool,
}

impl Choice {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            destructive: false,
        }
    }

    pub fn destructive(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            destructive: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Modal {
    /// A vertical list of answers, not a blind y/n. The cursor starts on
    /// the safe one.
    Confirm {
        title: String,
        body: String,
        choices: Vec<Choice>,
        at: usize,
    },
    /// Free text with a filtered list of suggestions under it: the branch
    /// name, and the branches that already exist.
    Input {
        title: String,
        prompt: String,
        value: String,
        suggestions: Vec<String>,
        at: usize,
    },
    /// What went wrong, wrapped. The status line is one line and git puts
    /// its reason at the end of a long one: "contains modified or
    /// untracked files" is exactly the part a flash throws away.
    Error { title: String, body: String },
    /// Every key, in full.
    Help,
    /// `:` — the k9s command bar.
    Command { value: String },
    /// Which agent to look at. Shown at startup only when nothing settled
    /// it, and any time through `:agent`.
    AgentPicker {
        agents: Vec<String>,
        at: usize,
        reason: Option<String>,
    },
    /// Which editor `enter` opens a worktree with. The last entry is not an
    /// editor but a way out of the list: a command line typed by hand.
    EditorPicker { editors: Vec<String>, at: usize },
}

/// The last row of the editor picker, which opens an input instead of
/// settling on a name.
pub const CUSTOM: &str = "custom…";

impl Modal {
    /// Move within whatever list this modal has.
    pub fn move_by(&mut self, delta: isize) {
        let (at, len) = match self {
            Self::Confirm { at, choices, .. } => (at, choices.len()),
            Self::Input {
                at, suggestions, ..
            } => (at, suggestions.len()),
            Self::AgentPicker { at, agents, .. } => (at, agents.len()),
            Self::EditorPicker { at, editors } => (at, editors.len()),
            Self::Help | Self::Command { .. } | Self::Error { .. } => return,
        };
        if len == 0 {
            *at = 0;
            return;
        }
        let next = (*at as isize + delta).rem_euclid(len as isize);
        *at = next as usize;
    }
}

/// A box of the given size, centred, never larger than what it sits on.
pub fn centred(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}

/// How many existing branches the input modal offers at once.
const SUGGESTIONS: usize = 8;

const HELP: &[(&str, &str)] = &[
    ("↑ ↓ / k j", "move"),
    ("← → / h l", "fold, unfold"),
    ("enter", "worktree: open with a fresh session"),
    ("", "session: bind it and open"),
    ("", "[…more]: load older sessions"),
    ("ctrl-o", "start a session in its own terminal"),
    ("ctrl-w", "create a worktree; on a session, hand it over"),
    ("ctrl-d", "delete a session, or remove a worktree"),
    ("ctrl-p", "purge: mark rows with space, enter to remove"),
    ("ctrl-l", "reload"),
    ("/", "filter — fuzzy; \x27 = ^ $ ! as in fzf"),
    (":", "command bar — :agent, :editor, :help, :q"),
    ("?", "this"),
    ("esc", "close, or quit"),
];

pub fn render(f: &mut Frame, area: Rect, modal: &Modal) {
    match modal {
        Modal::Confirm {
            title,
            body,
            choices,
            at,
        } => confirm(f, area, title, body, choices, *at),
        Modal::Input {
            title,
            prompt,
            value,
            suggestions,
            at,
        } => {
            input(f, area, title, prompt, value, suggestions, *at);
        }
        Modal::Error { title, body } => error(f, area, title, body),
        Modal::Help => help(f, area),
        Modal::Command { value } => command(f, area, value),
        Modal::AgentPicker { agents, at, reason } => {
            picker(f, area, "Agent", agents, *at, reason.as_deref());
        }
        Modal::EditorPicker { editors, at } => picker(f, area, "Editor", editors, *at, None),
    }
}

fn confirm(f: &mut Frame, area: Rect, title: &str, body: &str, choices: &[Choice], at: usize) {
    let rect = centred(area, 64, choices.len() as u16 + 6);
    frame(
        f,
        rect,
        title,
        theme::modal_border(),
        "↑↓ choose · enter · esc",
    );
    let mut lines = vec![
        Line::from(Span::raw(body.to_string())),
        Line::from(Span::raw("")),
    ];
    for (i, c) in choices.iter().enumerate() {
        let marker = if i == at { "▸ " } else { "  " };
        let style = if c.destructive {
            theme::danger()
        } else {
            theme::header_value()
        };
        let line = Line::from(vec![
            Span::raw(marker),
            Span::styled(c.label.clone(), style),
        ]);
        lines.push(if i == at {
            line.style(theme::cursor())
        } else {
            line
        });
    }
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        inset(rect),
    );
}

/// Wide enough for a git error with an absolute path in it.
const ERROR_WIDTH: u16 = 72;

fn error(f: &mut Frame, area: Rect, title: &str, body: &str) {
    let width = ERROR_WIDTH.min(area.width);
    // `inset` eats two columns each side.
    let height = wrapped_lines(body, width.saturating_sub(4)) + 2;
    let rect = centred(area, width, height);
    frame(f, rect, title, theme::danger(), "esc");
    f.render_widget(
        Paragraph::new(body.to_string())
            .style(theme::danger())
            .wrap(Wrap { trim: false }),
        inset(rect),
    );
}

/// How tall the wrapped text will be. A word longer than the line — a
/// path, every time — is broken rather than left to overflow, so counting
/// characters is close enough, and one spare line covers the rest.
fn wrapped_lines(text: &str, width: u16) -> u16 {
    let w = width.max(1) as usize;
    let n: usize = text
        .lines()
        .map(|l| l.chars().count().div_ceil(w).max(1))
        .sum();
    u16::try_from(n + 1).unwrap_or(u16::MAX)
}

/// The branch name, with the branches that already exist under it. This is
/// what the bash version needed a whole fzf reload to do.
fn input(
    f: &mut Frame,
    area: Rect,
    title: &str,
    prompt: &str,
    value: &str,
    suggestions: &[String],
    at: usize,
) {
    let shown = suggestions.len().min(SUGGESTIONS) as u16;
    let rect = centred(area, 64, shown + 6);
    frame(f, rect, title, theme::modal_border(), "enter · esc");
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0)])
        .split(inset(rect));
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!("{prompt} "), theme::header_label()),
            Span::styled(value.to_string(), theme::header_value()),
            Span::styled("▏", theme::header_key()),
        ])),
        rows[0],
    );
    let list: Vec<Line> = suggestions
        .iter()
        .take(SUGGESTIONS)
        .enumerate()
        .map(|(i, s)| {
            let l = Line::from(Span::styled(format!("  {s}"), theme::session()));
            if i == at { l.style(theme::cursor()) } else { l }
        })
        .collect();
    f.render_widget(Paragraph::new(list), rows[1]);
}

fn help(f: &mut Frame, area: Rect) {
    let rect = centred(area, 66, HELP.len() as u16 + 4);
    frame(f, rect, "Keys", theme::modal_border(), "esc");
    let lines: Vec<Line> = HELP
        .iter()
        .map(|(k, what)| {
            Line::from(vec![
                Span::styled(format!("  {k:<11}"), theme::header_key()),
                Span::styled((*what).to_string(), theme::header_label()),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inset(rect));
}

fn command(f: &mut Frame, area: Rect, value: &str) {
    let rect = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 3,
    };
    frame(f, rect, "Command", theme::modal_border(), "");
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(":", theme::header_key()),
            Span::styled(value.to_string(), theme::header_value()),
            Span::styled("▏", theme::header_key()),
        ])),
        inset(rect),
    );
}

fn picker(
    f: &mut Frame,
    area: Rect,
    title: &str,
    items: &[String],
    at: usize,
    reason: Option<&str>,
) {
    let extra = u16::from(reason.is_some()) * 2;
    let rect = centred(area, 54, items.len() as u16 + 4 + extra);
    frame(f, rect, title, theme::modal_border(), "↑↓ choose · enter");
    let mut lines = Vec::new();
    if let Some(r) = reason {
        lines.push(Line::from(Span::styled(r.to_string(), theme::danger())));
        lines.push(Line::from(Span::raw("")));
    }
    for (i, a) in items.iter().enumerate() {
        let marker = if i == at { "▸ " } else { "  " };
        let l = Line::from(vec![
            Span::raw(marker),
            Span::styled(a.clone(), theme::header_value()),
        ]);
        lines.push(if i == at { l.style(theme::cursor()) } else { l });
    }
    f.render_widget(Paragraph::new(lines), inset(rect));
}

fn frame(f: &mut Frame, rect: Rect, title: &str, style: ratatui::style::Style, hint: &str) {
    f.render_widget(Clear, rect);
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(style)
        .title(Span::styled(format!(" {title} "), theme::title()));
    if !hint.is_empty() {
        block = block.title_bottom(Line::from(Span::styled(format!(" {hint} "), theme::dim())));
    }
    f.render_widget(block, rect);
}

/// One column of breathing room inside the border.
fn inset(rect: Rect) -> Rect {
    Rect {
        x: rect.x + 2,
        y: rect.y + 1,
        width: rect.width.saturating_sub(4),
        height: rect.height.saturating_sub(2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn confirm() -> Modal {
        Modal::Confirm {
            title: "Remove worktree".into(),
            body: "hotfix has 3 sessions (1 running).".into(),
            choices: vec![
                Choice::new("Remove the worktree, keep the sessions"),
                Choice::destructive("Remove it and delete the sessions too"),
                Choice::new("Cancel"),
            ],
            at: 0,
        }
    }

    #[test]
    fn the_cursor_wraps_both_ways() {
        let mut m = confirm();
        m.move_by(-1);
        assert!(matches!(m, Modal::Confirm { at: 2, .. }));
        m.move_by(1);
        assert!(matches!(m, Modal::Confirm { at: 0, .. }));
    }

    #[test]
    fn an_empty_suggestion_list_does_not_divide_by_zero() {
        let mut m = Modal::Input {
            title: "t".into(),
            prompt: "branch>".into(),
            value: String::new(),
            suggestions: Vec::new(),
            at: 0,
        };
        m.move_by(1);
        assert!(matches!(m, Modal::Input { at: 0, .. }));
    }

    #[test]
    fn help_and_command_have_nothing_to_move_through() {
        let mut m = Modal::Help;
        m.move_by(1);
        assert_eq!(m, Modal::Help);
    }

    #[test]
    fn a_modal_never_grows_past_its_terminal() {
        let tiny = Rect {
            x: 0,
            y: 0,
            width: 20,
            height: 4,
        };
        let r = centred(tiny, 64, 20);
        assert!(r.width <= tiny.width);
        assert!(r.height <= tiny.height);
        assert!(r.x >= tiny.x && r.y >= tiny.y);
    }

    #[test]
    fn a_confirm_draws_its_body_and_every_choice() {
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 20)).unwrap();
        term.draw(|f| render(f, f.area(), &confirm())).unwrap();
        let text = super::super::tests_support::flatten(term.backend());
        assert!(text.contains("Remove worktree"));
        assert!(text.contains("3 sessions"));
        assert!(text.contains("keep the sessions"));
        assert!(text.contains("Cancel"));
    }

    #[test]
    fn the_agent_picker_says_why_it_is_asking() {
        let m = Modal::AgentPicker {
            agents: vec!["claude".into(), "codex".into()],
            at: 0,
            reason: Some("codex is configured but not installed here".into()),
        };
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 20)).unwrap();
        term.draw(|f| render(f, f.area(), &m)).unwrap();
        let text = super::super::tests_support::flatten(term.backend());
        assert!(text.contains("not installed here"));
        assert!(text.contains("claude"));
    }

    /// The bug this box exists for: the reason is at the end of git's
    /// line, and a one-line status bar cuts it off.
    #[test]
    fn an_error_wraps_far_enough_to_show_the_reason() {
        let m = Modal::Error {
            title: "Remove worktree".into(),
            body: "git worktree remove /Users/someone/projects/a-rather-long-name/worktrees/feat \
                   failed: fatal: '/Users/someone/projects/a-rather-long-name/worktrees/feat' \
                   contains modified or untracked files, use --force to delete it"
                .into(),
        };
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
        term.draw(|f| render(f, f.area(), &m)).unwrap();
        let text = super::super::tests_support::flatten(term.backend());
        assert!(text.contains("Remove worktree"));
        assert!(
            text.contains("use --force to delete it"),
            "the reason was cut off:\n{text}"
        );
    }

    #[test]
    fn an_error_box_fits_a_short_terminal() {
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(24, 5)).unwrap();
        let m = Modal::Error {
            title: "Open".into(),
            body: "no editor configured".into(),
        };
        term.draw(|f| render(f, f.area(), &m)).unwrap();
    }

    #[test]
    fn help_lists_every_key_the_interface_answers() {
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 24)).unwrap();
        term.draw(|f| render(f, f.area(), &Modal::Help)).unwrap();
        let text = super::super::tests_support::flatten(term.backend());
        for key in ["ctrl-o", "ctrl-w", "ctrl-d", "ctrl-p", "ctrl-l"] {
            assert!(text.contains(key), "{key} missing from help");
        }
    }
}
