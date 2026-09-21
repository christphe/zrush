//! One place for every colour, so a change is a change here and nowhere
//! else.
//!
//! Two rules learned the hard way:
//!
//! - Nothing load-bearing is drawn in `DarkGray`. On a dark background it
//!   is close enough to invisible that labels, borders and paths simply
//!   vanish, leaving the coloured fragments floating with no structure.
//! - The selected row is drawn in ONE style. Reversing each span
//!   separately turns every column's colour into a different background,
//!   which reads as a row of coloured blocks rather than a cursor.
//!
//! Everything resolves through `ratatui::style::Color`, and nothing uses an
//! indexed colour past the first sixteen: a terminal with a small palette
//! degrades rather than breaks.
//!
//! ## No forced background
//!
//! k9s paints its own background so it looks the same everywhere. zrush
//! does not: the version this replaced inherited the terminal's background
//! and looked at home on whatever theme the user had chosen, and taking
//! that over would flatten it. Named ANSI colours are used instead — a
//! terminal theme has already tuned those to be readable on its own
//! background.
//!
//! Two choices still depend on which way that background goes, and they are
//! the whole of `Variant`.

use std::sync::atomic::{AtomicU8, Ordering};

use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Dark,
    Light,
}

static VARIANT: AtomicU8 = AtomicU8::new(0);

pub fn set(variant: Variant) {
    VARIANT.store(u8::from(variant == Variant::Light), Ordering::Relaxed);
}

pub fn variant() -> Variant {
    if VARIANT.load(Ordering::Relaxed) == 0 {
        Variant::Dark
    } else {
        Variant::Light
    }
}

/// `COLORFGBG` is what terminals that report anything report: `fg;bg`, with
/// the background as an ANSI index. 0-6 and 8 are dark, 7 and 15 are light.
/// Nothing says it, which is most terminals, means dark: that is what a
/// developer's terminal almost always is, and it is the case that was
/// tested.
pub fn detect() -> Variant {
    detect_from(std::env::var("COLORFGBG").ok().as_deref())
}

pub fn detect_from(colorfgbg: Option<&str>) -> Variant {
    let Some(v) = colorfgbg else {
        return Variant::Dark;
    };
    match v
        .rsplit(';')
        .next()
        .and_then(|b| b.trim().parse::<u8>().ok())
    {
        Some(7 | 15) => Variant::Light,
        _ => Variant::Dark,
    }
}

fn pick(dark: Color, light: Color) -> Color {
    match variant() {
        Variant::Dark => dark,
        Variant::Light => light,
    }
}

/// Secondary text: labels, paths, key descriptions. The one colour that
/// genuinely flips — `Gray` reads on dark, `DarkGray` reads on light, and
/// each is close to invisible on the other.
fn muted() -> Color {
    pick(Color::Gray, Color::DarkGray)
}

/// The text on the cursor bar, over `SELECT_BG`.
fn select_fg() -> Color {
    pick(Color::Black, Color::White)
}

/// Worktrees read as structure, sessions as content, trouble as trouble.
pub const WORKTREE: Color = Color::Cyan;
pub const SESSION: Color = Color::Magenta;
pub const ORPHANS: Color = Color::Red;
pub const MORE: Color = Color::Magenta;
pub const BADGE: Color = Color::Yellow;
pub const GIT: Color = Color::Green;

pub const BORDER: Color = Color::Blue;
pub const TITLE: Color = Color::White;
pub const HEADER_KEY: Color = Color::Cyan;
pub const HEADER_VALUE: Color = Color::White;
pub const LOGO: Color = Color::Blue;

/// The cursor row: one background, one foreground, for the whole line.
pub const SELECT_BG: Color = Color::Cyan;
pub const MARKED: Color = Color::Yellow;
pub const DANGER: Color = Color::Red;

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
    Style::default().fg(MORE)
}

pub fn badge() -> Style {
    Style::default().fg(BADGE)
}

pub fn git() -> Style {
    Style::default().fg(GIT)
}

pub fn path() -> Style {
    Style::default().fg(muted())
}

/// One style for the whole row, so the cursor is a bar and not a row of
/// coloured blocks.
pub fn cursor() -> Style {
    Style::default()
        .bg(SELECT_BG)
        .fg(select_fg())
        .add_modifier(Modifier::BOLD)
}

/// The characters a filter matched. Underlined rather than recoloured, so
/// it survives on top of whatever the row's own colour is, including the
/// cursor bar.
pub fn matched() -> Style {
    Style::default().add_modifier(Modifier::UNDERLINED | Modifier::BOLD)
}

pub fn marked() -> Style {
    Style::default().fg(MARKED).add_modifier(Modifier::BOLD)
}

pub fn header_label() -> Style {
    Style::default().fg(muted())
}

pub fn header_value() -> Style {
    Style::default()
        .fg(HEADER_VALUE)
        .add_modifier(Modifier::BOLD)
}

pub fn header_key() -> Style {
    Style::default().fg(HEADER_KEY).add_modifier(Modifier::BOLD)
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
    Style::default().fg(HEADER_KEY).add_modifier(Modifier::BOLD)
}

pub fn danger() -> Style {
    Style::default().fg(DANGER).add_modifier(Modifier::BOLD)
}

pub fn dim() -> Style {
    Style::default().fg(muted())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `DarkGray` on a dark background is what made the first version read
    /// as debris: the structure was drawn, and none of it could be seen.
    #[test]
    fn nothing_load_bearing_is_invisible_on_a_dark_background() {
        set(Variant::Dark);
        for (what, style) in [
            ("border", border()),
            ("header label", header_label()),
            ("path", path()),
            ("dim", dim()),
            ("title", title()),
        ] {
            assert_ne!(
                style.fg,
                Some(Color::DarkGray),
                "{what} is invisible on a dark theme"
            );
        }
    }

    #[test]
    fn the_muted_colour_flips_with_the_background() {
        set(Variant::Dark);
        let dark = path().fg;
        set(Variant::Light);
        let light = path().fg;
        set(Variant::Dark);
        assert_ne!(dark, light, "the one colour that must differ does not");
    }

    /// Painting our own background would flatten whatever theme the
    /// terminal has, which the version this replaced never did.
    #[test]
    fn only_the_cursor_paints_a_background() {
        set(Variant::Dark);
        for (what, style) in [
            ("worktree", worktree()),
            ("session", session()),
            ("badge", badge()),
            ("git", git()),
            ("path", path()),
            ("border", border()),
            ("header label", header_label()),
            ("header value", header_value()),
            ("title", title()),
        ] {
            assert_eq!(style.bg, None, "{what} paints over the terminal background");
        }
        assert!(cursor().bg.is_some());
    }

    #[test]
    fn a_terminal_that_says_nothing_is_assumed_dark() {
        // Most say nothing, and a developer's terminal almost always is.
        assert_eq!(detect_from(None), Variant::Dark);
    }

    #[test]
    fn colorfgbg_is_read_when_it_is_there() {
        assert_eq!(detect_from(Some("15;0")), Variant::Dark);
        assert_eq!(detect_from(Some("0;15")), Variant::Light);
        assert_eq!(detect_from(Some("default;7")), Variant::Light);
        assert_eq!(detect_from(Some("nonsense")), Variant::Dark);
    }

    #[test]
    fn the_cursor_sets_both_a_background_and_a_foreground() {
        let c = cursor();
        assert!(c.bg.is_some(), "without a background the row is not a bar");
        assert!(
            c.fg.is_some(),
            "without a foreground the text keeps its own colour"
        );
    }
}
