//! One place for every colour, so a change is a change here and nowhere
//! else.
//!
//! ## Nothing is painted, and nothing needs configuring
//!
//! k9s paints its own background so it looks the same everywhere. zrush
//! does not: the version this replaced inherited the terminal's, and looked
//! at home on whatever theme the user had. Every colour here is a named
//! ANSI one, which a terminal theme has already tuned to be readable on its
//! own background.
//!
//! Two of them cannot be named that way, because which name reads depends
//! on which way the background goes: `Gray` (ANSI 7) vanishes on white,
//! `DarkGray` (ANSI 8) vanishes on black. Both are avoided rather than
//! chosen between — there is a colour that works on both by construction,
//! and it is the terminal's own:
//!
//! - **Muted text** — labels, paths, key descriptions — is the default
//!   foreground with `DIM`. A terminal that ignores `DIM` draws ordinary
//!   text, which is worse-looking and still perfectly readable; naming a
//!   colour risks invisible.
//! - **The cursor bar** is `REVERSED`, which swaps the terminal's own two
//!   colours. Applied to the whole row at once — reversing each column
//!   separately turns every column's colour into a different background,
//!   which reads as a row of coloured blocks rather than a cursor.
//!
//! So there is no dark theme and no light theme, and nothing to pick
//! between them.
//!
//! Nothing uses an indexed colour past the first sixteen: a terminal with a
//! small palette degrades rather than breaks.

use ratatui::style::{Color, Modifier, Style};

/// Worktrees read as structure, sessions as content, trouble as trouble.
pub const WORKTREE: Color = Color::Cyan;
pub const SESSION: Color = Color::Magenta;
pub const ORPHANS: Color = Color::Red;
pub const MORE: Color = Color::Magenta;
pub const BADGE: Color = Color::Yellow;
pub const GIT: Color = Color::Green;

pub const BORDER: Color = Color::Blue;
pub const HEADER_KEY: Color = Color::Cyan;
pub const LOGO: Color = Color::Blue;
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
    muted()
}

/// One style for the whole row, so the cursor is a bar and not a row of
/// coloured blocks. Reversed rather than coloured: it borrows the
/// terminal's own two colours, so it reads on any background.
pub fn cursor() -> Style {
    Style::default().add_modifier(Modifier::REVERSED)
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
    muted()
}

/// The default foreground, dimmed: readable on any background, because a
/// terminal guarantees its own foreground is readable on its own.
fn muted() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

pub fn header_value() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
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
    Style::default().add_modifier(Modifier::BOLD)
}

pub fn modal_border() -> Style {
    Style::default().fg(HEADER_KEY).add_modifier(Modifier::BOLD)
}

pub fn danger() -> Style {
    Style::default().fg(DANGER).add_modifier(Modifier::BOLD)
}

pub fn dim() -> Style {
    muted()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Gray` and `DarkGray` are the two that flip: each is close to
    /// invisible on the background the other suits. Naming either one is
    /// how the first version ended up unreadable.
    #[test]
    fn neither_of_the_two_background_dependent_greys_is_named() {
        for (what, style) in [
            ("border", border()),
            ("header label", header_label()),
            ("header value", header_value()),
            ("path", path()),
            ("dim", dim()),
            ("title", title()),
            ("cursor", cursor()),
        ] {
            assert!(
                !matches!(style.fg, Some(Color::Gray | Color::DarkGray)),
                "{what} names a grey that depends on the background"
            );
        }
    }

    /// Painting a background would flatten whatever theme the terminal has,
    /// which the version this replaced never did.
    #[test]
    fn nothing_paints_a_background() {
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
            ("cursor", cursor()),
        ] {
            assert_eq!(style.bg, None, "{what} paints over the terminal background");
        }
    }

    #[test]
    fn muted_text_borrows_the_terminals_own_foreground() {
        // Not a named colour: the terminal's, dimmed.
        assert_eq!(path().fg, None);
        assert!(path().add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn the_cursor_borrows_both_of_the_terminals_colours() {
        let c = cursor();
        assert!(c.add_modifier.contains(Modifier::REVERSED));
        assert_eq!(c.fg, None);
        assert_eq!(c.bg, None);
    }
}
