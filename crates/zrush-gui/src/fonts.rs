//! The fonts the window draws with.
//!
//! Text is set in `Inter`, drawn for screens and readable small, with a
//! semibold cut for titles and names. Code is set in `JetBrains Mono`.
//! egui's own fonts stay behind them for what neither has, and `JetBrains
//! Mono` sits behind the text face too: it has what an agent's answer or
//! zrush puts on screen and `Inter` lacks, `●` and `◌` for sessions, `└─`
//! for tree lines, `╭ ╮ ╰ ╯` for the boxes a terminal draws.
//!
//! Both are under the SIL Open Font License 1.1, which travels with them:
//! `assets/fonts/OFL-Inter.txt` and `assets/fonts/OFL-JetBrainsMono.txt`.

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily, FontId};

const TEXT: &str = "inter";
const TEXT_STRONG: &str = "inter-semibold";
const MONO: &str = "jetbrains-mono";
/// Inter again, with room between its lines: for reading, not for rows.
const READING: &str = "inter-reading";

/// `JetBrains Mono` again, its lines as tall as the reading face's: a word in
/// code sits on the line of the text around it, even when it wraps.
const MONO_READING: &str = "jetbrains-mono-reading";

/// The sizes a conversation reads at: text, and code within it.
pub const READING_SIZE: f32 = 14.0;
pub const READING_MONO_SIZE: f32 = 13.0;

/// The room added between lines of running text, in ems. Inter declares
/// none, which suits a list of rows and cramps a paragraph.
const READING_GAP: f32 = 0.45;

const TEXT_TTF: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
const TEXT_STRONG_TTF: &[u8] = include_bytes!("../assets/fonts/Inter-SemiBold.ttf");
/// The variant without ligatures: egui does not draw them anyway.
const MONO_TTF: &[u8] = include_bytes!("../assets/fonts/JetBrainsMonoNL-Regular.ttf");

pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(definitions());
}

/// How glyph coverage turns into ink: a little heavier than egui's own,
/// as macOS's font smoothing makes text. Without it, text drawn by egui
/// reads thin and soft beside every other window, the more so on a screen
/// that is not Retina. Light text on dark is spread less: it already
/// looks heavier.
pub fn smoothing(dark: bool) -> egui::epaint::FontColorTransferFunction {
    egui::epaint::FontColorTransferFunction::Gamma(if dark { 0.42 } else { 0.65 })
}

/// The family a conversation reads in.
pub fn reading_family() -> FontFamily {
    FontFamily::Name(READING.into())
}

/// The reading face at `size`, or the plain one until the fonts are in.
pub fn reading(ctx: &egui::Context, size: f32) -> FontId {
    let family = reading_family();
    let loaded = ctx.fonts(|f| f.definitions().families.contains_key(&family));
    if loaded {
        FontId::new(size, family)
    } else {
        FontId::proportional(size)
    }
}

/// The code face a conversation reads in.
pub fn mono_reading(ctx: &egui::Context) -> FontId {
    let family = FontFamily::Name(MONO_READING.into());
    let loaded = ctx.fonts(|f| f.definitions().families.contains_key(&family));
    if loaded {
        FontId::new(READING_MONO_SIZE, family)
    } else {
        FontId::monospace(READING_MONO_SIZE)
    }
}

/// A face's height from the top of its tallest letter to the bottom of its
/// lowest, in ems, read where a layout reads it: the `OS/2` typographic
/// metrics when the face says to use them, `hhea` otherwise.
fn height_em(ttf: &[u8]) -> Option<f32> {
    let u16_at = |at: usize| Some(u16::from_be_bytes(ttf.get(at..at + 2)?.try_into().ok()?));
    let i16_at = |at: usize| Some(i16::from_be_bytes(ttf.get(at..at + 2)?.try_into().ok()?));
    let table = |tag: &[u8; 4]| {
        (0..usize::from(u16_at(4)?)).find_map(|i| {
            let rec = 12 + 16 * i;
            if ttf.get(rec..rec + 4)? != tag {
                return None;
            }
            Some(u32::from_be_bytes(ttf.get(rec + 8..rec + 12)?.try_into().ok()?) as usize)
        })
    };
    let per_em = f32::from(u16_at(table(b"head")? + 18)?);
    let os2 = table(b"OS/2")?;
    let use_typo = u16_at(os2 + 62)? & (1 << 7) != 0;
    let (asc, desc) = if use_typo {
        (i16_at(os2 + 68)?, i16_at(os2 + 70)?)
    } else {
        let hhea = table(b"hhea")?;
        (i16_at(hhea + 4)?, i16_at(hhea + 6)?)
    };
    Some((f32::from(asc) - f32::from(desc)) / per_em)
}

/// `ttf` with `em` of line gap declared, where a layout reads it: `hhea`
/// and the `OS/2` table's typographic metrics. `None` when the file is not
/// laid out as expected; checksums are not kept, nothing here reads them.
fn with_line_gap(ttf: &[u8], em: f32) -> Option<Vec<u8>> {
    let u16_at =
        |b: &[u8], at: usize| Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?));
    let u32_at =
        |b: &[u8], at: usize| Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?));
    let table = |tag: &[u8; 4]| {
        (0..usize::from(u16_at(ttf, 4)?)).find_map(|i| {
            let rec = 12 + 16 * i;
            (ttf.get(rec..rec + 4)? == tag).then(|| u32_at(ttf, rec + 8))?
        })
    };
    let head = table(b"head")? as usize;
    let hhea = table(b"hhea")? as usize;
    let os2 = table(b"OS/2")? as usize;
    let per_em = f32::from(u16_at(ttf, head + 18)?);
    let gap = i16::try_from((per_em * em).round() as i64)
        .ok()?
        .to_be_bytes();
    let mut out = ttf.to_vec();
    for at in [hhea + 8, os2 + 72] {
        out.get_mut(at..at + 2)?.copy_from_slice(&gap);
    }
    Some(out)
}

/// The family titles, names and primary buttons are set in.
pub fn strong_family() -> FontFamily {
    FontFamily::Name(TEXT_STRONG.into())
}

/// The strong face at `size`, or the plain one until the fonts are in:
/// they are set on the first frame and only load for the next.
pub fn strong(ctx: &egui::Context, size: f32) -> FontId {
    let family = strong_family();
    let loaded = ctx.fonts(|f| f.definitions().families.contains_key(&family));
    if loaded {
        FontId::new(size, family)
    } else {
        FontId::proportional(size)
    }
}

fn definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in [
        (TEXT, TEXT_TTF),
        (TEXT_STRONG, TEXT_STRONG_TTF),
        (MONO, MONO_TTF),
    ] {
        fonts
            .font_data
            .insert(name.into(), Arc::new(FontData::from_static(bytes)));
    }
    // egui's fonts, as they come, are the fallbacks of every family.
    let fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let with = |first: &str| {
        let mut chain = vec![first.to_string()];
        chain.extend(fallback.iter().cloned());
        chain.push(MONO.into());
        chain
    };
    // The gap lands below the letters; half of it above them centres them
    // in their line, as egui centres a word in code or a list's marker.
    let reading = with_line_gap(TEXT_TTF, READING_GAP).map_or_else(
        || FontData::from_static(TEXT_TTF),
        |b| {
            FontData::from_owned(b).tweak(egui::FontTweak {
                y_offset_factor: READING_GAP / 2.0,
                ..Default::default()
            })
        },
    );
    fonts.font_data.insert(READING.into(), Arc::new(reading));
    fonts.families.insert(reading_family(), with(READING));
    // Code in a conversation: as tall a line as the text, the same way.
    let line = height_em(TEXT_TTF).map(|h| (h + READING_GAP) * READING_SIZE);
    let mono_gap = line
        .zip(height_em(MONO_TTF))
        .map(|(line, h)| line / READING_MONO_SIZE - h);
    let mono = mono_gap
        .and_then(|gap| Some((gap, with_line_gap(MONO_TTF, gap)?)))
        .map_or_else(
            || FontData::from_static(MONO_TTF),
            |(gap, b)| {
                FontData::from_owned(b).tweak(egui::FontTweak {
                    y_offset_factor: gap / 2.0,
                    ..Default::default()
                })
            },
        );
    fonts.font_data.insert(MONO_READING.into(), Arc::new(mono));
    fonts.families.insert(
        FontFamily::Name(MONO_READING.into()),
        vec![MONO_READING.into()],
    );
    fonts.families.insert(FontFamily::Proportional, with(TEXT));
    fonts.families.insert(strong_family(), with(TEXT_STRONG));
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, MONO.into());
    fonts
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What zrush and its agents put on screen that the text face lacks.
    const DRAWN: &str = "●◌▸▾└├─│╭╮╰╯↑↓·…";

    fn has(ctx: &egui::Context, font: &FontId, s: &str) -> bool {
        ctx.fonts_mut(|f| f.has_glyphs(font, s))
    }

    #[test]
    fn every_symbol_zrush_draws_has_a_glyph() {
        let ctx = egui::Context::default();
        install(&ctx);
        // One frame loads the fonts; its textures go nowhere here.
        let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
        out.textures_delta.clear();
        for font in [
            FontId::proportional(14.0),
            FontId::new(14.0, strong_family()),
            FontId::monospace(13.0),
        ] {
            for c in DRAWN.chars() {
                assert!(has(&ctx, &font, &c.to_string()), "no {c} in {font:?}");
            }
        }
    }

    #[test]
    fn text_is_inter_and_code_jetbrains_mono() {
        let f = definitions();
        assert_eq!(f.families[&FontFamily::Proportional][0], TEXT);
        assert_eq!(f.families[&strong_family()][0], TEXT_STRONG);
        assert_eq!(f.families[&FontFamily::Monospace][0], MONO);
    }

    #[test]
    fn smoothing_thickens_without_blotting() {
        use egui::epaint::FontColorTransferFunction::Gamma;
        for dark in [true, false] {
            let Gamma(g) = smoothing(dark) else {
                panic!("not a gamma");
            };
            assert!((0.35..1.0).contains(&g), "{g}");
        }
    }

    #[test]
    fn reading_text_has_room_between_its_lines() {
        let ctx = egui::Context::default();
        install(&ctx);
        let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
        out.textures_delta.clear();
        let (rows, reading) = ctx.fonts_mut(|f| {
            (
                f.row_height(&FontId::proportional(14.0)),
                f.row_height(&FontId::new(14.0, reading_family())),
            )
        });
        assert!(reading >= rows + 5.0, "{rows} then {reading}");
    }

    #[test]
    fn a_file_not_laid_out_as_a_font_is_left_alone() {
        assert!(with_line_gap(b"not a font", 0.4).is_none());
    }

    #[test]
    fn code_in_a_conversation_is_as_tall_a_line_as_its_text() {
        let ctx = egui::Context::default();
        install(&ctx);
        let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
        out.textures_delta.clear();
        let text = FontId::new(READING_SIZE, reading_family());
        let code = mono_reading(&ctx);
        let (t, c) = ctx.fonts_mut(|f| (f.row_height(&text), f.row_height(&code)));
        assert!((t - c).abs() <= 0.5, "text {t}, code {c}");
    }
}
