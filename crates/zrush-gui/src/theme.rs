//! Themes are files: a name, light or dark, and the colours of each part
//! of the window. Eight ship inside the binary; a file in the themes
//! folder adds one, or replaces a built-in of the same name.

use std::collections::BTreeMap;
use std::path::Path;

use egui::Color32;
use serde::Deserialize;

/// The themes that ship, by file name: what `gui_theme` names them by.
pub const BUILT_IN: &[(&str, &str)] = &[
    ("vscode-dark", include_str!("../themes/vscode-dark.toml")),
    ("vscode-light", include_str!("../themes/vscode-light.toml")),
    ("zed-dark", include_str!("../themes/zed-dark.toml")),
    ("zed-light", include_str!("../themes/zed-light.toml")),
    (
        "solarized-dark",
        include_str!("../themes/solarized-dark.toml"),
    ),
    (
        "solarized-light",
        include_str!("../themes/solarized-light.toml"),
    ),
    ("nord", include_str!("../themes/nord.toml")),
    ("dracula", include_str!("../themes/dracula.toml")),
];

/// What `auto` picks, and where a missing colour comes from.
const DEFAULT_DARK: &str = "vscode-dark";
const DEFAULT_LIGHT: &str = "vscode-light";

/// One colour per part of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub window_bg: Color32,
    pub panel_bg: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub border: Color32,
    pub selection: Color32,
    pub accent: Color32,
    pub live: Color32,
    pub resumable: Color32,
    pub warning: Color32,
    pub error: Color32,
    pub code_bg: Color32,
    pub user_turn: Color32,
    pub agent_turn: Color32,
}

impl Palette {
    /// The names a theme file uses under `[colors]`, in field order.
    pub const KEYS: [&'static str; 14] = [
        "window_bg",
        "panel_bg",
        "text",
        "muted",
        "border",
        "selection",
        "accent",
        "live",
        "resumable",
        "warning",
        "error",
        "code_bg",
        "user_turn",
        "agent_turn",
    ];

    fn slot(&mut self, key: &str) -> Option<&mut Color32> {
        Some(match key {
            "window_bg" => &mut self.window_bg,
            "panel_bg" => &mut self.panel_bg,
            "text" => &mut self.text,
            "muted" => &mut self.muted,
            "border" => &mut self.border,
            "selection" => &mut self.selection,
            "accent" => &mut self.accent,
            "live" => &mut self.live,
            "resumable" => &mut self.resumable,
            "warning" => &mut self.warning,
            "error" => &mut self.error,
            "code_bg" => &mut self.code_bg,
            "user_turn" => &mut self.user_turn,
            "agent_turn" => &mut self.agent_turn,
            _ => return None,
        })
    }

    /// Egui's own colours, the base a built-in default starts from.
    fn egui(dark: bool) -> Self {
        let v = if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        let text = v.text_color();
        Self {
            window_bg: v.panel_fill,
            panel_bg: v.window_fill,
            text,
            muted: v.weak_text_color(),
            border: v.widgets.noninteractive.bg_stroke.color,
            selection: v.selection.bg_fill,
            accent: v.hyperlink_color,
            live: v.selection.bg_fill,
            resumable: v.weak_text_color(),
            warning: v.warn_fg_color,
            error: v.error_fg_color,
            code_bg: v.code_bg_color,
            user_turn: v.hyperlink_color,
            agent_turn: text,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    /// The file name without `.toml`: what `gui_theme` names.
    pub name: String,
    /// What the settings window shows.
    pub label: String,
    pub dark: bool,
    pub palette: Palette,
}

#[derive(Deserialize)]
struct ThemeFile {
    label: String,
    dark: bool,
    #[serde(default)]
    colors: BTreeMap<String, String>,
}

/// `#rrggbb` or `#rrggbbaa`.
pub fn parse_colour(s: &str) -> Option<Color32> {
    let hex = s.strip_prefix('#')?;
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    match hex.len() {
        6 => Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?)),
        8 => Some(Color32::from_rgba_unmultiplied(
            byte(0)?,
            byte(2)?,
            byte(4)?,
            byte(6)?,
        )),
        _ => None,
    }
}

/// A theme from its file's text. A colour the file leaves out is `base`'s.
fn parse(name: &str, text: &str, base: impl Fn(bool) -> Palette) -> Result<Theme, String> {
    let file: ThemeFile = toml::from_str(text).map_err(|e| e.message().to_string())?;
    let mut palette = base(file.dark);
    for (key, value) in &file.colors {
        let colour = parse_colour(value)
            .ok_or_else(|| format!("{key} = \"{value}\" is not a #rrggbb colour"))?;
        *palette
            .slot(key)
            .ok_or_else(|| format!("no colour called {key}"))? = colour;
    }
    Ok(Theme {
        name: name.to_string(),
        label: file.label,
        dark: file.dark,
        palette,
    })
}

/// The built-ins, then every `*.toml` in `user_dir` (a missing folder is
/// no themes, not an error). A user file named like a built-in replaces
/// it. A file that does not parse is skipped and reported as
/// `"{file}: {reason}"`.
pub fn load_all(user_dir: &Path) -> (Vec<Theme>, Vec<String>) {
    let mut themes: Vec<Theme> = Vec::new();
    for (name, text) in BUILT_IN {
        // Built-ins name every colour; egui's are only the base for that.
        if let Ok(t) = parse(name, text, Palette::egui) {
            themes.push(t);
        }
    }
    let defaults = |dark: bool| {
        let want = if dark { DEFAULT_DARK } else { DEFAULT_LIGHT };
        themes
            .iter()
            .find(|t| t.name == want)
            .map_or_else(|| Palette::egui(dark), |t| t.palette)
    };
    let mut errors = Vec::new();
    let mut added: Vec<Theme> = Vec::new();
    let mut files: Vec<_> = std::fs::read_dir(user_dir)
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    files.sort();
    for path in files {
        if path.extension().is_none_or(|x| x != "toml") {
            continue;
        }
        let file = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let name = path
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let parsed = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| parse(&name, &text, defaults));
        match parsed {
            Ok(t) => added.push(t),
            Err(e) => errors.push(format!("{file}: {e}")),
        }
    }
    for t in added {
        if let Some(slot) = themes.iter_mut().find(|b| b.name == t.name) {
            *slot = t;
        } else {
            themes.push(t);
        }
    }
    (themes, errors)
}

/// The theme `name` asks for: `auto`, or a name nothing answers to, takes
/// the default for the system's light or dark mode.
pub fn pick<'a>(themes: &'a [Theme], name: &str, os_dark: bool) -> &'a Theme {
    let fallback = if os_dark { DEFAULT_DARK } else { DEFAULT_LIGHT };
    themes
        .iter()
        .find(|t| t.name == name)
        .or_else(|| themes.iter().find(|t| t.name == fallback))
        .unwrap_or(&themes[0])
}

/// Dress egui in the theme.
pub fn apply(ctx: &egui::Context, t: &Theme) {
    let p = &t.palette;
    let mut v = if t.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.override_text_color = Some(p.text);
    v.weak_text_color = Some(p.muted);
    v.panel_fill = p.window_bg;
    v.window_fill = p.panel_bg;
    v.window_stroke = egui::Stroke::new(1.0, p.border);
    v.faint_bg_color = p.panel_bg;
    v.extreme_bg_color = p.code_bg;
    v.code_bg_color = p.code_bg;
    v.hyperlink_color = p.accent;
    v.warn_fg_color = p.warning;
    v.error_fg_color = p.error;
    v.selection.bg_fill = p.selection;
    v.selection.stroke = egui::Stroke::new(1.0, p.text);
    v.widgets.noninteractive.bg_fill = p.window_bg;
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, p.border);
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, p.text);
    v.widgets.inactive.weak_bg_fill = p.panel_bg;
    v.widgets.inactive.bg_fill = p.panel_bg;
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, p.border);
    v.widgets.hovered.weak_bg_fill = p.selection;
    v.widgets.hovered.bg_fill = p.selection;
    v.widgets.active.weak_bg_fill = p.selection;
    v.widgets.active.bg_fill = p.selection;
    ctx.set_visuals(v);
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("zrush-palette"), *p));
}

/// The palette `apply` last set, for the parts egui does not colour
/// itself: status dots, the turn labels.
pub fn palette(ctx: &egui::Context) -> Option<Palette> {
    ctx.data(|d| d.get_temp(egui::Id::new("zrush-palette")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn by<'a>(themes: &'a [Theme], name: &str) -> &'a Theme {
        themes.iter().find(|t| t.name == name).unwrap()
    }

    #[test]
    fn every_builtin_parses_and_names_every_colour() {
        let td = tempfile::TempDir::new().unwrap();
        let (themes, errors) = load_all(td.path());
        assert_eq!(errors, Vec::<String>::new());
        assert_eq!(themes.len(), 8);
        for t in BUILT_IN {
            let file: ThemeFile = toml::from_str(t.1).unwrap();
            assert_eq!(
                file.colors.len(),
                Palette::KEYS.len(),
                "{} misses a colour",
                t.0
            );
        }
    }

    #[test]
    fn a_user_theme_with_a_builtins_name_replaces_it() {
        let td = tempfile::TempDir::new().unwrap();
        std::fs::write(
            td.path().join("nord.toml"),
            "label = \"Mine\"\ndark = true\n[colors]\n",
        )
        .unwrap();
        let (themes, _) = load_all(td.path());
        assert_eq!(by(&themes, "nord").label, "Mine");
        assert_eq!(themes.len(), 8);
    }

    #[test]
    fn a_user_theme_with_a_new_name_is_added() {
        let td = tempfile::TempDir::new().unwrap();
        std::fs::write(
            td.path().join("mine.toml"),
            "label = \"Mine\"\ndark = false\n[colors]\naccent = \"#ff0000\"\n",
        )
        .unwrap();
        let (themes, _) = load_all(td.path());
        assert_eq!(themes.len(), 9);
        assert_eq!(
            by(&themes, "mine").palette.accent,
            egui::Color32::from_rgb(255, 0, 0)
        );
    }

    #[test]
    fn a_missing_colour_falls_back_to_the_default() {
        let td = tempfile::TempDir::new().unwrap();
        std::fs::write(
            td.path().join("half.toml"),
            "label = \"Half\"\ndark = true\n[colors]\nwindow_bg = \"#000000\"\n",
        )
        .unwrap();
        let (themes, _) = load_all(td.path());
        let half = by(&themes, "half");
        assert_eq!(half.palette.window_bg, egui::Color32::BLACK);
        assert_eq!(half.palette.text, by(&themes, "vscode-dark").palette.text);
    }

    #[test]
    fn a_broken_file_is_reported_and_skipped() {
        let td = tempfile::TempDir::new().unwrap();
        std::fs::write(td.path().join("broken.toml"), "not = [toml").unwrap();
        let (themes, errors) = load_all(td.path());
        assert_eq!(errors.len(), 1);
        assert!(errors[0].starts_with("broken.toml: "), "{}", errors[0]);
        assert_eq!(themes.len(), 8);
    }

    #[test]
    fn a_bad_colour_is_an_error_naming_it() {
        let td = tempfile::TempDir::new().unwrap();
        std::fs::write(
            td.path().join("bad.toml"),
            "label = \"Bad\"\ndark = true\n[colors]\ntext = \"blue\"\n",
        )
        .unwrap();
        let (_, errors) = load_all(td.path());
        assert!(errors[0].contains("text"), "{}", errors[0]);
    }

    #[test]
    fn an_unknown_colour_name_is_an_error() {
        let td = tempfile::TempDir::new().unwrap();
        std::fs::write(
            td.path().join("typo.toml"),
            "label = \"Typo\"\ndark = true\n[colors]\ntxet = \"#ffffff\"\n",
        )
        .unwrap();
        let (_, errors) = load_all(td.path());
        assert!(errors[0].contains("txet"), "{}", errors[0]);
    }

    #[test]
    fn auto_follows_the_system() {
        let td = tempfile::TempDir::new().unwrap();
        let (t, _) = load_all(td.path());
        assert_eq!(pick(&t, "auto", true).name, "vscode-dark");
        assert_eq!(pick(&t, "auto", false).name, "vscode-light");
        assert_eq!(pick(&t, "nope", true).name, "vscode-dark");
        assert_eq!(pick(&t, "solarized-light", true).name, "solarized-light");
    }

    #[test]
    fn colours_read_with_or_without_alpha() {
        assert_eq!(
            parse_colour("#102030"),
            Some(egui::Color32::from_rgb(16, 32, 48))
        );
        assert_eq!(
            parse_colour("#10203040"),
            Some(egui::Color32::from_rgba_unmultiplied(16, 32, 48, 64))
        );
        assert_eq!(parse_colour("102030"), None);
        assert_eq!(parse_colour("#12345"), None);
    }
}
