//! The block at the top: where you are, what is loaded, which keys work.
//!
//! In the spirit of k9s — an info block, the keys beside it, a logo on the
//! right — but sized to its content. A fixed-width column truncates
//! ("◌ 1 resuma") the moment a repo has a few more sessions than the one it
//! was eyeballed on.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::theme;

/// Five, because that is how tall the logo is; the info block uses four
/// of them.
pub const HEIGHT: u16 = 5;

/// Below this there is no room for the logo; below `MIN_FOR_KEYS` there is
/// none for the key grid either, and the info block takes what is left.
const MIN_FOR_LOGO: u16 = 140;
const MIN_FOR_KEYS: u16 = 64;

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

impl Info<'_> {
    fn rows(&self) -> [(&'static str, String); 4] {
        let counts = if self.loaded {
            format!(
                "{} · ●{} live · ◌{} resumable",
                self.worktrees, self.live, self.resumable
            )
        } else {
            format!("{} · loading…", self.worktrees)
        };
        [
            ("Repo", self.repo.to_string()),
            ("Branch", self.branch.to_string()),
            ("Agent", self.agent.to_string()),
            ("Worktrees", counts),
        ]
    }
}

/// Five rows, padded to a rectangle so the column it sits in is the width
/// it claims. Raw strings: it is mostly backslashes.
const LOGO: [&str; 5] = [
    r" ______  ______   __  __   ______   __  __   ",
    r"/\___  \/\  == \ /\ \/\ \ /\  ___\ /\ \_\ \  ",
    r"\/_/  /_\ \  __< \ \ \_\ \\ \___  \\ \  __ \ ",
    r"  /\____\\ \_\ \_\\ \_____\\/\_____\\ \_\ \_\",
    r"  \/____/ \/_/ /_/ \/_____/ \/_____/ \/_/\/_/",
];
const LOGO_WIDTH: u16 = 45;

/// Two columns of `<key> action`, the way k9s lists them.
const KEYS: [[(&str, &str); 2]; 3] = [
    [("enter", "open"), ("^o", "new session")],
    [("^w", "worktree"), ("^d", "delete")],
    [("/", "filter"), ("?", "help")],
];

/// The widest `<key> action ` pair, so the two columns line up.
fn key_column_width() -> u16 {
    KEYS.iter()
        .flatten()
        .map(|(k, a)| k.len() + a.len() + 4)
        .max()
        .unwrap_or(20) as u16
}

pub fn render(f: &mut Frame, area: Rect, info: &Info<'_>) {
    let rows = info.rows();
    let label_width = rows.iter().map(|(l, _)| l.len()).max().unwrap_or(9) + 2;
    let info_width = rows
        .iter()
        .map(|(_, v)| label_width + UnicodeWidthStr::width(v.as_str()))
        .max()
        .unwrap_or(30) as u16
        + 2;

    let keys_width = key_column_width() * 2;
    let want_keys = area.width >= MIN_FOR_KEYS;
    let want_logo = area.width >= MIN_FOR_LOGO;

    let mut constraints = vec![Constraint::Length(info_width.min(area.width))];
    if want_keys {
        constraints.push(Constraint::Length(keys_width));
    }
    constraints.push(Constraint::Min(0));
    if want_logo {
        constraints.push(Constraint::Length(LOGO_WIDTH));
    }
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);

    let left: Vec<Line> = rows
        .iter()
        .map(|(label, value)| {
            Line::from(vec![
                Span::styled(format!("{label:<label_width$}"), theme::header_label()),
                Span::styled(value.clone(), theme::header_value()),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(left), cols[0]);

    if want_keys {
        let width = key_column_width() as usize;
        let middle: Vec<Line> = KEYS
            .iter()
            .map(|pairs| {
                let mut spans = Vec::new();
                for (key, action) in pairs {
                    let pad = width - key.len() - 3;
                    spans.push(Span::styled(format!("<{key}>"), theme::header_key()));
                    spans.push(Span::styled(
                        format!(" {action:<pad$}"),
                        theme::header_label(),
                    ));
                }
                Line::from(spans)
            })
            .collect();
        f.render_widget(Paragraph::new(middle), cols[1]);
    }

    if want_logo {
        let logo: Vec<Line> = LOGO
            .iter()
            .map(|l| Line::from(Span::styled(*l, theme::logo())))
            .collect();
        f.render_widget(Paragraph::new(logo), cols[cols.len() - 1]);
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    fn info(live: usize, resumable: usize, loaded: bool) -> Info<'static> {
        Info {
            repo: "wt",
            branch: "main",
            agent: "claude",
            worktrees: 2,
            live,
            resumable,
            loaded,
        }
    }

    pub fn draw_at(width: u16) -> String {
        draw(&info(1, 1, true), width)
    }

    fn draw(info: &Info<'_>, width: u16) -> String {
        let mut term =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, HEIGHT)).unwrap();
        term.draw(|f| render(f, f.area(), info)).unwrap();
        super::super::tests_support::flatten(term.backend())
    }

    #[test]
    fn the_logo_is_a_rectangle_of_the_width_it_claims() {
        for l in LOGO {
            assert_eq!(UnicodeWidthStr::width(l), LOGO_WIDTH as usize);
        }
    }

    #[test]
    fn the_logo_is_a_rectangle() {
        for l in LOGO {
            assert_eq!(
                UnicodeWidthStr::width(l),
                LOGO_WIDTH as usize,
                "ragged: |{l}|"
            );
        }
    }

    #[test]
    fn the_header_is_tall_enough_for_the_logo() {
        assert!(HEIGHT as usize >= LOGO.len());
    }

    #[test]
    fn the_counts_are_never_truncated() {
        // "◌ 1 resuma" is what a fixed-width column does the moment a repo
        // has more sessions than the one it was eyeballed on.
        let text = draw(&info(12, 137, true), 200);
        assert!(text.contains("137 resumable"), "cut off in:\n{text}");
    }

    #[test]
    fn every_key_keeps_its_description() {
        let text = draw(&info(1, 1, true), 200);
        for (_, action) in KEYS.iter().flatten() {
            assert!(text.contains(action), "'{action}' missing from:\n{text}");
        }
    }

    #[test]
    fn the_labels_are_drawn() {
        let text = draw(&info(1, 1, true), 200);
        for label in ["Repo", "Branch", "Agent", "Worktrees"] {
            assert!(text.contains(label), "'{label}' missing from:\n{text}");
        }
    }

    #[test]
    fn a_narrow_header_drops_the_logo_before_the_keys() {
        let wide = draw(&info(1, 1, true), 200);
        assert!(wide.contains(LOGO[2]), "no logo at 200 columns:\n{wide}");
        let mid = draw(&info(1, 1, true), 100);
        assert!(
            !mid.contains(LOGO[2]),
            "the logo should be gone at 100 columns"
        );
        assert!(
            mid.contains("open"),
            "the keys should survive at 100 columns"
        );
    }

    #[test]
    fn a_very_narrow_header_keeps_only_where_you_are() {
        let text = draw(&info(1, 1, true), 40);
        assert!(text.contains("claude"));
    }

    #[test]
    fn the_counts_say_loading_rather_than_zero() {
        // Zero resumable and "not scanned yet" are different facts, and the
        // difference is the whole point of loading concurrently.
        let text = draw(&info(0, 0, false), 120);
        assert!(text.contains("loading"));
        assert!(!text.contains("0 resumable"));
    }
}
