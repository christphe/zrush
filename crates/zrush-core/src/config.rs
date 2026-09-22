use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Result, ZrushError};

/// Where zrush reads and writes. Every consumer takes a `&Dirs` rather than
/// reaching for `$HOME`, so a test can point it at a temporary directory.
#[derive(Debug, Clone)]
pub struct Dirs {
    pub config: PathBuf,
    pub cache: PathBuf,
    pub claude_projects: PathBuf,
}

impl Dirs {
    pub fn from_env() -> Result<Self> {
        let home = dirs::home_dir().ok_or_else(|| ZrushError::msg("no home directory"))?;
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map_or_else(|| home.join(".config"), PathBuf::from)
            .join("zrush");
        let cache = std::env::var_os("XDG_CACHE_HOME")
            .map_or_else(|| home.join(".cache"), PathBuf::from)
            .join("zrush")
            .join("sessions");
        Ok(Self {
            config,
            cache,
            claude_projects: home.join(".claude").join("projects"),
        })
    }

    pub fn config_file(&self) -> PathBuf {
        self.config.join("config.toml")
    }

    /// The shell-syntax file the bash version sourced.
    pub fn legacy_config_file(&self) -> PathBuf {
        self.config.join("config")
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    pub default_repo: Option<PathBuf>,
    /// Which agent to start on. Unset means ask, unless only one is
    /// installed, in which case there is nothing to ask.
    pub agent: Option<String>,
    pub editor: String,
    pub editor_cmd: Vec<String>,
    pub terminal: Vec<String>,
    pub titles: bool,
    pub status: bool,
    pub resumable_max: usize,
    pub resumable_scan: usize,
    pub more_step: usize,
    pub title_width: usize,
    pub preview_turns: usize,
    /// `auto`, `dark` or `light`. Only decides the handful of colours that
    /// depend on which way the terminal background goes; zrush never paints
    /// a background of its own.
    pub theme: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_repo: None,
            agent: None,
            editor: "zed".into(),
            editor_cmd: Vec::new(),
            terminal: Vec::new(),
            titles: true,
            status: true,
            resumable_max: 5,
            resumable_scan: 40,
            more_step: 20,
            title_width: 48,
            preview_turns: 14,
            theme: "auto".into(),
        }
    }
}

impl Config {
    /// The file, then the environment. A missing `config.toml` with a legacy
    /// shell `config` beside it is imported once and written back as TOML,
    /// so an existing setup survives the upgrade. The old file is left alone.
    pub fn load(dirs: &Dirs) -> Result<Self> {
        let path = dirs.config_file();
        let mut cfg = if path.exists() {
            let text = std::fs::read_to_string(&path)?;
            toml::from_str(&text).map_err(|e| ZrushError::Config(e.to_string()))?
        } else if dirs.legacy_config_file().exists() {
            let text = std::fs::read_to_string(dirs.legacy_config_file())?;
            let imported = from_legacy_shell(&text);
            imported.save(dirs)?;
            imported
        } else {
            Self::default()
        };
        cfg.apply_env(|k| std::env::var(k).ok());
        Ok(cfg)
    }

    pub fn save(&self, dirs: &Dirs) -> Result<()> {
        std::fs::create_dir_all(&dirs.config)?;
        let text = toml::to_string_pretty(self).map_err(|e| ZrushError::Config(e.to_string()))?;
        std::fs::write(dirs.config_file(), text)?;
        Ok(())
    }

    pub fn apply_env(&mut self, get: impl Fn(&str) -> Option<String>) {
        if let Some(v) = get("ZRUSH_DEFAULT_REPO").filter(|v| !v.is_empty()) {
            self.default_repo = Some(PathBuf::from(v));
        }
        if let Some(v) = get("ZRUSH_AGENT").filter(|v| !v.is_empty()) {
            self.agent = Some(v);
        }
        if let Some(v) = get("ZRUSH_EDITOR").filter(|v| !v.is_empty()) {
            self.editor = v;
        }
        if let Some(v) = get("ZRUSH_EDITOR_CMD") {
            self.editor_cmd = split_words(&v);
        }
        if let Some(v) = get("ZRUSH_TERMINAL") {
            self.terminal = split_words(&v);
        }
        set_bool(&mut self.titles, get("ZRUSH_TITLES").as_deref());
        set_bool(&mut self.status, get("ZRUSH_STATUS").as_deref());
        set_usize(
            &mut self.resumable_max,
            get("ZRUSH_RESUMABLE_MAX").as_deref(),
        );
        set_usize(
            &mut self.resumable_scan,
            get("ZRUSH_RESUMABLE_SCAN").as_deref(),
        );
        set_usize(&mut self.more_step, get("ZRUSH_MORE_STEP").as_deref());
        set_usize(&mut self.title_width, get("ZRUSH_TITLE_WIDTH").as_deref());
        set_usize(
            &mut self.preview_turns,
            get("ZRUSH_PREVIEW_TURNS").as_deref(),
        );
    }

    /// `editor_cmd` replaces the whole command line. Otherwise the three
    /// editors we know about get `-n`: a worktree nested inside another one
    /// is otherwise swallowed by the parent project's window.
    pub fn editor_command(&self) -> Vec<String> {
        if !self.editor_cmd.is_empty() {
            return self.editor_cmd.clone();
        }
        Self::command_for(&self.editor)
    }

    /// The command line a bare editor name means, with no config in hand:
    /// what `:editor code` has to turn into.
    pub fn command_for(editor: &str) -> Vec<String> {
        match editor {
            "" => vec!["zed".into(), "-n".into()],
            e @ ("zed" | "cursor" | "code") => vec![e.into(), "-n".into()],
            other => vec![other.into()],
        }
    }
}

/// The editors that get a `-n`, in the order the picker offers them.
pub const KNOWN_EDITORS: &[&str] = &["zed", "cursor", "code"];

fn set_bool(slot: &mut bool, v: Option<&str>) {
    if let Some(v) = v {
        *slot = v != "0";
    }
}

fn set_usize(slot: &mut usize, v: Option<&str>) {
    if let Some(n) = v.and_then(|v| v.parse().ok()) {
        *slot = n;
    }
}

/// Shell-ish word splitting: parentheses and quotes dropped, whitespace
/// separating. Enough for the values the old config file held.
fn split_words(v: &str) -> Vec<String> {
    v.trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .split_whitespace()
        .map(|w| w.trim_matches(['"', '\'']).to_string())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Best-effort import of the shell-syntax config: every `ZRUSH_X=...`
/// assignment, ignoring comments and anything else.
pub fn from_legacy_shell(text: &str) -> Config {
    let mut seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || !line.starts_with("ZRUSH_") {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            seen.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    let mut cfg = Config::default();
    cfg.apply_env(|k| seen.get(k).cloned());
    cfg
}

/// A `Dirs` rooted at `base`, for tests.
pub fn dirs_under(base: &Path) -> Dirs {
    Dirs {
        config: base.join("config"),
        cache: base.join("cache"),
        claude_projects: base.join("projects"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_shell_version() {
        let c = Config::default();
        assert_eq!(c.editor, "zed");
        assert_eq!(c.resumable_max, 5);
        assert_eq!(c.resumable_scan, 40);
        assert_eq!(c.more_step, 20);
        assert_eq!(c.title_width, 48);
        assert_eq!(c.preview_turns, 14);
        assert!(c.titles);
        assert!(c.status);
    }

    #[test]
    fn the_default_agent_comes_from_the_file_or_the_environment() {
        let mut c = Config::default();
        assert_eq!(c.agent, None);
        c.apply_env(|k| (k == "ZRUSH_AGENT").then(|| "codex".to_string()));
        assert_eq!(c.agent.as_deref(), Some("codex"));
    }

    #[test]
    fn env_beats_the_file() {
        let mut c = Config {
            title_width: 10,
            ..Config::default()
        };
        c.apply_env(|k| (k == "ZRUSH_TITLE_WIDTH").then(|| "99".to_string()));
        assert_eq!(c.title_width, 99);
    }

    #[test]
    fn zero_disables_the_boolean_knobs() {
        let mut c = Config::default();
        c.apply_env(|k| (k == "ZRUSH_TITLES").then(|| "0".to_string()));
        assert!(!c.titles);
    }

    #[test]
    fn a_known_editor_gets_the_new_window_flag() {
        let c = Config {
            editor: "code".into(),
            ..Config::default()
        };
        assert_eq!(c.editor_command(), vec!["code", "-n"]);
    }

    #[test]
    fn editor_cmd_wins_over_editor() {
        let c = Config {
            editor: "zed".into(),
            editor_cmd: vec!["code".into(), "--reuse-window".into()],
            ..Config::default()
        };
        assert_eq!(c.editor_command(), vec!["code", "--reuse-window"]);
    }

    #[test]
    fn an_unknown_editor_is_run_as_is() {
        let c = Config {
            editor: "hx".into(),
            ..Config::default()
        };
        assert_eq!(c.editor_command(), vec!["hx"]);
    }

    #[test]
    fn the_legacy_shell_config_is_imported() {
        let c = from_legacy_shell(
            "# a comment\n\
             ZRUSH_EDITOR=cursor\n\
             ZRUSH_TITLE_WIDTH=60\n\
             ZRUSH_EDITOR_CMD=(code --reuse-window)\n\
             ZRUSH_TERMINAL=(\"open\" -a Ghostty)\n\
             ZRUSH_STATUS=0\n",
        );
        assert_eq!(c.editor, "cursor");
        assert_eq!(c.title_width, 60);
        assert_eq!(c.editor_cmd, vec!["code", "--reuse-window"]);
        assert_eq!(c.terminal, vec!["open", "-a", "Ghostty"]);
        assert!(!c.status);
    }

    #[test]
    fn a_saved_config_round_trips() {
        let td = tempfile::TempDir::new().unwrap();
        let d = dirs_under(td.path());
        let c = Config {
            title_width: 33,
            editor: "hx".into(),
            ..Config::default()
        };
        c.save(&d).unwrap();
        let text = std::fs::read_to_string(d.config_file()).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back, c);
    }
}
