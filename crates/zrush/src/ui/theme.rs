//! One place for every colour, so a change is a change here and nowhere
//! else.
//!
//! Everything resolves through `ratatui::style::Color`, and nothing uses an
//! indexed colour past the first sixteen: a terminal with a small palette
//! degrades rather than breaks.

use ratatui::style::{Color, Modifier, Style};

/// Worktrees read as structure, sessions as content, trouble as trouble.
pub const WORKTREE: Color = Color::Cyan;
pub const SESSION: Color = Color::Magenta;
pub const ORPHANS: Color = Color::Red;
pub const MORE: Color = Color::Magenta;
pub const BADGE: Color = Color::Yellow;
pub const GIT: Color = Color::Green;
pub const PATH: Color = Color::DarkGray;

pub const LIVE: Color = Color::Green;
pub const RESUMABLE: Color = Color::DarkGray;

pub const HEADER_KEY: Color = Color::Cyan;
pub const HEADER_LABEL: Color = Color::DarkGray;
pub const HEADER_VALUE: Color = Color::White;
pub const LOGO: Color = Color::Blue;

pub const BORDER: Color = Color::DarkGray;
pub const TITLE: Color = Color::White;

pub const MODAL_BORDER: Color = Color::Cyan;
pub const DANGER: Color = Color::Red;
pub const MARKED: Color = Color::Yellow;

pub fn worktree() -> Style {
    Style::default().fg(WORKTREE).add_modifier(Modifier::BOLD)
}

pub fn session() -> Style {
    Style::default().fg(SESSION)
}

pub fn orphans() -> Style {
    Style::default().fg(ORPHANS).add_modifier(Modifier::BOLD)
}

pub fn more() -> Style {
    Style::default().fg(MORE).add_modifier(Modifier::DIM)
}

pub fn badge() -> Style {
    Style::default().fg(BADGE)
}

pub fn git() -> Style {
    Style::default().fg(GIT)
}

pub fn path() -> Style {
    Style::default().fg(PATH).add_modifier(Modifier::DIM)
}

/// The row under the cursor. Reversed rather than recoloured, so it stays
/// visible whatever the row's own colour is.
pub fn cursor() -> Style {
    Style::default().add_modifier(Modifier::REVERSED)
}

pub fn marked() -> Style {
    Style::default().fg(MARKED).add_modifier(Modifier::BOLD)
}

pub fn header_label() -> Style {
    Style::default().fg(HEADER_LABEL)
}

pub fn header_value() -> Style {
    Style::default()
        .fg(HEADER_VALUE)
        .add_modifier(Modifier::BOLD)
}

pub fn header_key() -> Style {
    Style::default().fg(HEADER_KEY)
}

pub fn logo() -> Style {
    Style::default().fg(LOGO).add_modifier(Modifier::BOLD)
}

pub fn border() -> Style {
    Style::default().fg(BORDER)
}

pub fn title() -> Style {
    Style::default().fg(TITLE).add_modifier(Modifier::BOLD)
}

pub fn modal_border() -> Style {
    Style::default().fg(MODAL_BORDER)
}

pub fn danger() -> Style {
    Style::default().fg(DANGER).add_modifier(Modifier::BOLD)
}

pub fn dim() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}
