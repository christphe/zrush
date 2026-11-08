//! The settings window: every setting the terminal's settings screen has,
//! written to the same `config.toml`.

use std::collections::HashMap;

use zrush_core::actions;
use zrush_core::app::Zrush;
use zrush_core::config::{Config, KNOWN_EDITORS, Kind, SETTINGS};

use crate::theme::Theme;

/// A change that went through, for the window to pass on to every tab.
pub enum Changed {
    Config(Config),
}

/// What the window holds between frames: whether it is open, the text
/// being typed in each field, and each field's last refusal.
#[derive(Default)]
pub struct SettingsState {
    pub open: bool,
    drafts: HashMap<String, String>,
    errors: HashMap<String, String>,
    custom_editor: String,
    /// The config as saved (file, then environment), read when the window
    /// needs it and again after each change. Not the service's live copy:
    /// that one carries the window's own overrides.
    saved: Option<Config>,
}

/// Draw the window when it is open. `z` is the service the change goes
/// through; the window applies what comes back to the other tabs.
pub fn show(
    ctx: &egui::Context,
    state: &mut SettingsState,
    z: &Zrush,
    themes: &[Theme],
) -> Option<Changed> {
    if !state.open {
        return None;
    }
    let mut open = true;
    let mut changed = None;
    egui::Window::new("Settings")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size([600.0, 640.0])
        .anchor(egui::Align2::CENTER_TOP, [0.0, 72.0])
        .show(ctx, |ui| {
            let cfg = state
                .saved
                .get_or_insert_with(|| Config::load(z.dirs()).unwrap_or_else(|_| z.config()))
                .clone();
            egui::ScrollArea::vertical()
                .max_height(560.0)
                .show(ui, |ui| {
                    section(ui, "Appearance");
                    if let Some(c) = theme_row(ui, state, z, &cfg, themes) {
                        changed = Some(c);
                    }
                    for (title, keys) in SECTIONS {
                        ui.add_space(10.0);
                        section(ui, title);
                        egui::Grid::new(("settings", *title))
                            .num_columns(2)
                            .spacing([20.0, 10.0])
                            .min_col_width(170.0)
                            .show(ui, |ui| {
                                for key in *keys {
                                    let Some((_, kind)) = SETTINGS.iter().find(|(k, _)| k == key)
                                    else {
                                        continue;
                                    };
                                    if let Some(c) = row(ui, state, z, &cfg, key, *kind) {
                                        changed = Some(c);
                                    }
                                    ui.end_row();
                                }
                            });
                    }
                });
        });
    state.open = open;
    if changed.is_some() {
        state.saved = None;
    }
    changed
}

/// The settings, grouped the way someone looks for them.
const SECTIONS: &[(&str, &[&str])] = &[
    (
        "Editor and agent",
        &["editor", "editor_cmd", "terminal", "agent"],
    ),
    (
        "The list",
        &[
            "titles",
            "status",
            "resumable_max",
            "more_step",
            "resumable_scan",
            "title_width",
            "preview_turns",
        ],
    ),
    ("Starting up", &["default_repo", "update_check"]),
];

/// What a setting is called on screen; its `config.toml` name shows on
/// hover.
fn label_for(key: &str) -> &'static str {
    match key {
        "editor" => "Editor",
        "editor_cmd" => "Editor command",
        "terminal" => "Terminal",
        "agent" => "Agent",
        "titles" => "Session titles",
        "status" => "Git status column",
        "resumable_max" => "Sessions per worktree",
        "more_step" => "Show more by",
        "resumable_scan" => "Transcripts scanned",
        "title_width" => "Title width (terminal)",
        "preview_turns" => "Preview turns (terminal)",
        "default_repo" => "Default repository",
        "update_check" => "Check for updates",
        _ => "Setting",
    }
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.label(
        egui::RichText::new(title.to_uppercase())
            .small()
            .strong()
            .color(ui.visuals().weak_text_color()),
    );
    ui.add_space(2.0);
}

/// The name on the left of a row, with the file's name for it on hover.
fn name(ui: &mut egui::Ui, key: &str) -> egui::Response {
    ui.label(label_for(key))
        .on_hover_text(format!("{key} in config.toml"))
}

fn row(
    ui: &mut egui::Ui,
    state: &mut SettingsState,
    z: &Zrush,
    cfg: &Config,
    key: &str,
    kind: Kind,
) -> Option<Changed> {
    let result = match kind {
        Kind::Flag => {
            ui.label("");
            let mut on = cfg.show(key) == "on";
            if ui
                .checkbox(&mut on, label_for(key))
                .on_hover_text(format!("{key} in config.toml"))
                .changed()
            {
                Some(z.edit_config(|c| {
                    let _ = c.toggle(key);
                }))
            } else {
                None
            }
        }
        Kind::Editor => {
            let label = name(ui, key);
            editor(ui, state, z, cfg, label.id)
        }
        Kind::Agent => {
            name(ui, key);
            let current = cfg.agent.clone().unwrap_or_default();
            let mut picked = None;
            ui.horizontal(|ui| {
                for a in z.agents() {
                    if ui.selectable_label(current == a.id(), a.id()).clicked() {
                        picked = Some(a.id());
                    }
                }
            });
            picked.map(|id| actions::switch_agent(z, id))
        }
        Kind::Number | Kind::Words | Kind::Path => {
            let label = name(ui, key);
            let mut out = None;
            ui.vertical(|ui| {
                let draft = state
                    .drafts
                    .entry(key.to_string())
                    .or_insert_with(|| cfg.show(key));
                let mut edit = egui::TextEdit::singleline(draft).desired_width(300.0);
                if state.errors.contains_key(key) {
                    edit = edit.text_color(ui.visuals().error_fg_color);
                }
                let r = ui.add(edit).labelled_by(label.id);
                if r.lost_focus() {
                    let value = draft.clone();
                    if value != cfg.show(key) {
                        let mut refused = None;
                        let saved = z.edit_config(|c| refused = c.set(key, &value).err());
                        out = Some(refused.map_or(saved, Err));
                    }
                }
                if let Some(e) = state.errors.get(key) {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!("{}: {e}", label_for(key)),
                    );
                }
            });
            out
        }
    };
    match result? {
        Ok(cfg) => {
            state.errors.remove(key);
            state.drafts.remove(key);
            Some(Changed::Config(cfg))
        }
        Err(e) => {
            state.errors.insert(key.to_string(), e.to_string());
            None
        }
    }
}

/// The theme, by its label; `auto` follows the system's light or dark.
fn theme_row(
    ui: &mut egui::Ui,
    state: &mut SettingsState,
    z: &Zrush,
    cfg: &Config,
    themes: &[Theme],
) -> Option<Changed> {
    let mut picked: Option<String> = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        if ui
            .selectable_label(cfg.gui_theme == "auto", "Auto")
            .on_hover_text("Follows the system's light or dark mode")
            .clicked()
        {
            picked = Some("auto".into());
        }
        for t in themes {
            if ui
                .selectable_label(cfg.gui_theme == t.name, &t.label)
                .clicked()
            {
                picked = Some(t.name.clone());
            }
        }
    });
    let name = picked?;
    match z.edit_config(|c| c.gui_theme = name) {
        Ok(cfg) => Some(Changed::Config(cfg)),
        Err(e) => {
            state.errors.insert("gui_theme".into(), e.to_string());
            None
        }
    }
}

/// A known editor by name, or a whole command line typed in.
fn editor(
    ui: &mut egui::Ui,
    state: &mut SettingsState,
    z: &Zrush,
    cfg: &Config,
    label: egui::Id,
) -> Option<zrush_core::error::Result<Config>> {
    let mut out = None;
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            for name in KNOWN_EDITORS {
                let current = cfg.editor_cmd.is_empty() && cfg.editor == *name;
                if ui.selectable_label(current, *name).clicked() {
                    out = Some(z.set_editor(name).map(|()| z.config()));
                }
            }
        });
        ui.horizontal(|ui| {
            let r = ui
                .add(
                    egui::TextEdit::singleline(&mut state.custom_editor)
                        .hint_text("or a command line, e.g. code -n")
                        .desired_width(240.0),
                )
                .labelled_by(label);
            let go = ui.button("Use").clicked()
                || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            if go {
                let words: Vec<String> = state
                    .custom_editor
                    .split_whitespace()
                    .map(str::to_string)
                    .collect();
                if !words.is_empty() {
                    out = Some(z.set_editor_command(&words).map(|()| z.config()));
                }
            }
        });
    });
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use zrush_core::config::{Config, dirs_under};
    use zrush_core::host::RecordingHost;

    use crate::app::GuiApp;
    use crate::testing::{git, scratch_repo};

    fn two_tabs(td: &tempfile::TempDir) -> GuiApp {
        let first = scratch_repo(td);
        let second = zrush_core::paths::real(td.path()).join("second");
        std::fs::create_dir_all(&second).unwrap();
        git(&second, &["init", "-q"]);
        git(&second, &["config", "user.email", "t@example.invalid"]);
        git(&second, &["config", "user.name", "zrush tests"]);
        std::fs::write(second.join("g.txt"), "x").unwrap();
        git(&second, &["add", "g.txt"]);
        git(&second, &["commit", "-qm", "init"]);
        let mut app = GuiApp::new(
            dirs_under(td.path()),
            Config::default(),
            vec![first, second],
            Box::new(|_| Box::new(RecordingHost::default())),
            Arc::new(|| {}),
        );
        app.open_settings();
        app
    }

    fn harness(app: GuiApp) -> Harness<'static, GuiApp> {
        let mut h = Harness::builder()
            .with_size([1200.0, 900.0])
            .build_eframe(|_cc| app);
        h.run_steps(3);
        h
    }

    fn saved(td: &tempfile::TempDir) -> Option<Config> {
        Config::load_file(&dirs_under(td.path())).ok()
    }

    #[test]
    fn a_flag_toggled_here_is_saved_and_every_tab_sees_it() {
        let td = tempfile::TempDir::new().unwrap();
        let mut h = harness(two_tabs(&td));
        h.get_by_label("Session titles").click_accesskit();
        h.run_steps(3);
        assert!(!saved(&td).unwrap().titles);
        assert_eq!(h.state().tab_count(), 2);
        for t in h.state().tabs() {
            assert!(!t.zrush().unwrap().config().titles, "{}", t.title);
        }
    }

    #[test]
    fn a_refused_number_shows_the_error_and_saves_nothing() {
        let td = tempfile::TempDir::new().unwrap();
        let before = saved(&td).map(|c| c.resumable_max);
        let mut h = harness(two_tabs(&td));
        let field = h.get_by_label("Sessions per worktree");
        field.focus();
        field.type_text("lots");
        h.key_press(egui::Key::Enter);
        h.run_steps(3);
        assert!(
            h.query_by_label_contains("Sessions per worktree:")
                .is_some()
        );
        assert_eq!(saved(&td).map(|c| c.resumable_max), before);
    }

    #[test]
    fn settings_show_what_is_saved_not_the_windows_own_override() {
        // The window reads whole conversations by overriding preview_turns
        // for itself; the settings must show, and keep, the saved value.
        let td = tempfile::TempDir::new().unwrap();
        let h = harness(two_tabs(&td));
        let field = h.get_by_label("Preview turns (terminal)");
        assert_eq!(field.value().as_deref(), Some("14"));
    }

    #[test]
    fn picking_a_theme_saves_it() {
        let td = tempfile::TempDir::new().unwrap();
        let mut h = harness(two_tabs(&td));
        h.get_by_label("Nord").click();
        h.run_steps(2);
        assert_eq!(saved(&td).unwrap().gui_theme, "nord");
    }

    #[test]
    fn picking_an_editor_saves_it() {
        let td = tempfile::TempDir::new().unwrap();
        let mut h = harness(two_tabs(&td));
        h.get_by_label("code").click();
        h.run_steps(2);
        assert_eq!(saved(&td).unwrap().editor, "code");
    }
}
