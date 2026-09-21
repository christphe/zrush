//! The block at the top: where you are, what is loaded, which keys work.
//!
//! In the manner of k9s: an info block on the left, the keys in a grid
//! beside it, a logo on the right. It never scrolls away, so the answer to
//! "which agent am I looking at" is always on screen.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::theme;

pub const HEIGHT: u16 = 4;

pub struct Info<'a> {
    pub repo: &'a str,
    pub branch: &'a str,
    pub agent: &'a str,
    pub worktrees: usize,
    pub live: usize,
    pub resumable: usize,
    /// False until the transcript scan has answered, so the counts can say
    /// they are still arriving instead of lying about being zero.
    pub loaded: bool,
}

const LOGO: [&str; 3] = [
    "╱╴ ╭─╮ ╭─╮ ╷ ╷ ╭─╴ ╷ ╷",
    "╱  ╭╯  ├┬╯ │ │ ╰─╮ ├─┤",
    "╱  ╰─╴ ╵╰╴ ╰─╯ ╶─╯ ╵ ╵",
];

/// Two columns of `<key> action`, the way k9s lists them.
const KEYS: [[(&str, &str); 2]; 3] = [
    [("enter", "open"), ("ctrl-o", "new session")],
    [("ctrl-w", "worktree"), ("ctrl-d", "delete")],
    [("/", "filter"), ("?", "help")],
];

fn labelled(label: &str, value: String) -> Line<'_> {
    Line::from(vec![
        Span::styled(format!("{label:<11}"), theme::header_label()),
        Span::styled(value, theme::header_value()),
    ])
}

fn keys_column(pairs: &[(&str, &str)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (key, action) in pairs {
        spans.push(Span::styled(format!("<{key}>"), theme::header_key()));
        spans.push(Span::styled(
            format!(" {action:<14}"),
            theme::header_label(),
        ));
    }
    Line::from(spans)
}

pub fn render(f: &mut Frame, area: Rect, info: &Info<'_>) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(34),
            Constraint::Min(20),
            Constraint::Length(24),
        ])
        .split(area);

    let counts = if info.loaded {
        format!(
            "{}  ● {} live  ◌ {} resumable",
            info.worktrees, info.live, info.resumable
        )
    } else {
        format!("{}  loading…", info.worktrees)
    };
    let left = vec![
        labelled("Repo", info.repo.to_string()),
        labelled("Branch", info.branch.to_string()),
        labelled("Agent", info.agent.to_string()),
        labelled("Worktrees", counts),
    ];
    f.render_widget(Paragraph::new(left), cols[0]);

    let middle: Vec<Line> = KEYS.iter().map(|row| keys_column(row)).collect();
    f.render_widget(Paragraph::new(middle), cols[1]);

    let logo: Vec<Line> = LOGO
        .iter()
        .map(|l| Line::from(Span::styled(*l, theme::logo())))
        .collect();
    f.render_widget(Paragraph::new(logo), cols[2]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_logo_is_a_rectangle() {
        let w = LOGO[0].chars().count();
        for l in LOGO {
            assert_eq!(l.chars().count(), w, "logo lines must be the same width");
        }
    }

    #[test]
    fn the_header_is_tall_enough_for_what_it_draws() {
        assert!(HEIGHT as usize >= LOGO.len());
        assert!(HEIGHT as usize >= KEYS.len());
    }

    #[test]
    fn the_counts_say_loading_rather_than_zero() {
        // Zero resumable and "not scanned yet" are different facts, and the
        // difference is the whole point of loading concurrently.
        let info = Info {
            repo: "r",
            branch: "main",
            agent: "claude",
            worktrees: 2,
            live: 0,
            resumable: 0,
            loaded: false,
        };
        let mut term =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, HEIGHT)).unwrap();
        term.draw(|f| render(f, f.area(), &info)).unwrap();
        let text = super::super::tests_support::flatten(term.backend());
        assert!(text.contains("loading"));
        assert!(!text.contains("0 resumable"));
    }
}
