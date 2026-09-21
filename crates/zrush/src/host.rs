//! The terminal's answer to `zrush_core::host::Host`: real processes.
//!
//! An editor is a command line with a path appended. An agent gets a
//! terminal window of its own, which on macOS means writing a script and
//! handing it to whatever opens that kind of file — there is no portable
//! way to tell a specific terminal to run something.

use std::path::Path;
use std::process::Command;

use zrush_core::config::Config;
use zrush_core::error::{Result, ZrushError};
use zrush_core::host::Host;

pub struct ProcessHost {
    editor: Vec<String>,
    terminal: Vec<String>,
}

impl ProcessHost {
    pub fn new(cfg: &Config) -> Self {
        // Empty means `open`, which hands the script to whatever app owns
        // that file type.
        let terminal = if cfg.terminal.is_empty() {
            vec![default_opener().to_string()]
        } else {
            cfg.terminal.clone()
        };
        Self {
            editor: cfg.editor_command(),
            terminal,
        }
    }
}

impl Host for ProcessHost {
    fn open_workspace(&self, path: &Path) -> Result<()> {
        let (bin, args) = self
            .editor
            .split_first()
            .ok_or_else(|| ZrushError::msg("no editor configured"))?;
        Command::new(bin)
            .args(args)
            .arg(path)
            .status()
            .map_err(|_| ZrushError::msg(format!("{bin} not found (install its shell command)")))?;
        Ok(())
    }

    fn run_agent(&self, cwd: &Path, command: &[String]) -> Result<()> {
        let dir = std::env::temp_dir().join(format!("zrush-term.{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let script = dir.join(script_name());
        std::fs::write(&script, script_body(cwd, command, &dir))?;
        make_executable(&script)?;

        let (bin, args) = self
            .terminal
            .split_first()
            .ok_or_else(|| ZrushError::msg("no terminal configured"))?;
        Command::new(bin)
            .args(args)
            .arg(&script)
            .status()
            .map_err(|_| {
                let _ = std::fs::remove_dir_all(&dir);
                ZrushError::msg(format!("{bin} could not open a terminal"))
            })?;
        Ok(())
    }
}

/// Markers an editor may have inherited from the session that launched it.
/// Without clearing them, Claude Code writes no transcript — and a session
/// with no transcript can never be resumed, which is the whole point.
const INHERITED: &[&str] = &[
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_PID",
    "CLAUDECODE",
    "CLAUDE_EFFORT",
    "AI_AGENT",
];

#[cfg(windows)]
fn default_opener() -> &'static str {
    "cmd"
}

#[cfg(not(windows))]
fn default_opener() -> &'static str {
    "open"
}

#[cfg(windows)]
fn script_name() -> &'static str {
    "session.cmd"
}

#[cfg(not(windows))]
fn script_name() -> &'static str {
    "session.command"
}

#[cfg(windows)]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(not(windows))]
fn make_executable(path: &Path) -> Result<()> {
    let mut perms = std::fs::metadata(path)?.permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

/// The script deletes itself once the agent exits, so nothing accumulates
/// in the temp directory.
#[cfg(not(windows))]
pub fn script_body(cwd: &Path, command: &[String], cleanup: &Path) -> String {
    use std::fmt::Write as _;

    let mut s = String::from("#!/bin/sh\n");
    for var in INHERITED {
        let _ = writeln!(s, "unset {var}");
    }
    let _ = writeln!(s, "cd {} || exit 1", shell_quote(&cwd.to_string_lossy()));
    let quoted: Vec<String> = command.iter().map(|a| shell_quote(a)).collect();
    let _ = writeln!(s, "{}", quoted.join(" "));
    let _ = writeln!(s, "rm -rf {}", shell_quote(&cleanup.to_string_lossy()));
    s
}

/// Untested on real hardware: Windows has no CI runner with a terminal to
/// try it on. Compiled and unit-tested, and documented as best-effort.
#[cfg(windows)]
pub fn script_body(cwd: &Path, command: &[String], cleanup: &Path) -> String {
    use std::fmt::Write as _;

    let mut s = String::from("@echo off\r\n");
    for var in INHERITED {
        let _ = write!(s, "set {var}=\r\n");
    }
    let _ = write!(s, "cd /d \"{}\" || exit /b 1\r\n", cwd.display());
    let _ = write!(s, "{}\r\n", command.join(" "));
    let _ = write!(s, "rmdir /s /q \"{}\"\r\n", cleanup.display());
    s
}

/// Single quotes, with embedded quotes escaped the POSIX way. A repo path
/// may hold anything, including a quote.
#[cfg(not(windows))]
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_editor_command_comes_from_the_config() {
        let cfg = Config {
            editor: "code".into(),
            ..Config::default()
        };
        assert_eq!(ProcessHost::new(&cfg).editor, vec!["code", "-n"]);
    }

    #[test]
    fn an_unset_terminal_falls_back_to_the_platform_opener() {
        let h = ProcessHost::new(&Config::default());
        assert_eq!(h.terminal, vec![default_opener()]);
    }

    #[cfg(not(windows))]
    #[test]
    fn the_script_clears_every_inherited_marker() {
        let body = script_body(
            Path::new("/tmp/wt"),
            &["claude".into()],
            Path::new("/tmp/d"),
        );
        for var in INHERITED {
            assert!(body.contains(&format!("unset {var}")), "{var} not cleared");
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn the_script_cds_runs_and_cleans_up_after_itself() {
        let body = script_body(
            Path::new("/tmp/wt"),
            &["claude".into(), "--resume".into(), "abc".into()],
            Path::new("/tmp/d"),
        );
        assert!(body.contains("cd '/tmp/wt' || exit 1"));
        assert!(body.contains("'claude' '--resume' 'abc'"));
        assert!(body.contains("rm -rf '/tmp/d'"));
    }

    #[cfg(not(windows))]
    #[test]
    fn a_quote_in_a_path_cannot_break_out_of_the_script() {
        let body = script_body(
            Path::new("/tmp/it's here"),
            &["claude".into()],
            Path::new("/tmp/d"),
        );
        assert!(body.contains(r"cd '/tmp/it'\''s here'"));
    }
}
