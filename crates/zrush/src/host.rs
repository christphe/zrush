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
        raise(bin);
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

/// Bringing the editor to the front.
///
/// On macOS, opening a window and activating the application are two
/// different things: `zed -n <path>` makes the window and leaves the app
/// where it was, so the worktree opens behind whatever you were looking at
/// and it reads as nothing having happened. `-n` itself is not optional —
/// without it a worktree nested inside another one is swallowed by the
/// parent project's window — so the raise is a second step.
///
/// Elsewhere the window manager decides, and a new window normally takes
/// focus by itself, so there is nothing here to do.
#[cfg(target_os = "macos")]
mod focus {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// Failure is ignored: the editor did open, and not being frontmost is
    /// not worth an error.
    pub fn raise(editor_bin: &str) {
        let Some(resolved) = locate(editor_bin) else {
            return;
        };
        let Some(bundle) = bundle_of(&resolved) else {
            return;
        };
        let _ = Command::new("open").arg("-a").arg(bundle).status();
    }

    /// The `.app` a command lives inside, if any.
    ///
    /// Derived rather than guessed from the command's name:
    /// `/usr/local/bin/zed` resolves to
    /// `/Applications/Zed.app/Contents/MacOS/cli`, and the same walk finds
    /// `Visual Studio Code.app` from `code`. An editor that is not in a
    /// bundle simply has nothing to raise.
    fn bundle_of(resolved: &Path) -> Option<&Path> {
        resolved
            .ancestors()
            .find(|a| a.extension().is_some_and(|e| e == "app"))
    }

    /// Where a command actually is, following symlinks.
    fn locate(bin: &str) -> Option<PathBuf> {
        let direct = Path::new(bin);
        if direct.components().count() > 1 {
            return direct.canonicalize().ok();
        }
        let paths = std::env::var_os("PATH")?;
        std::env::split_paths(&paths).find_map(|dir| dir.join(bin).canonicalize().ok())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_bundle_is_derived_from_where_the_cli_really_lives() {
            assert_eq!(
                bundle_of(Path::new("/Applications/Zed.app/Contents/MacOS/cli")),
                Some(Path::new("/Applications/Zed.app"))
            );
            assert_eq!(
                bundle_of(Path::new(
                    "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code"
                )),
                Some(Path::new("/Applications/Visual Studio Code.app"))
            );
        }

        #[test]
        fn an_editor_outside_a_bundle_has_nothing_to_raise() {
            assert_eq!(bundle_of(Path::new("/usr/local/bin/hx")), None);
            assert_eq!(bundle_of(Path::new("/")), None);
        }

        #[test]
        fn the_innermost_bundle_wins() {
            assert_eq!(
                bundle_of(Path::new(
                    "/Applications/Outer.app/Contents/Inner.app/MacOS/cli"
                )),
                Some(Path::new("/Applications/Outer.app/Contents/Inner.app"))
            );
        }

        #[test]
        fn a_command_on_path_is_found() {
            assert!(locate("sh").is_some());
            assert!(locate("definitely-not-a-command-here").is_none());
        }
    }
}

#[cfg(target_os = "macos")]
use focus::raise;

/// Nothing to do: the window manager decides, and a new window normally
/// takes focus by itself.
#[cfg(not(target_os = "macos"))]
fn raise(_editor_bin: &str) {}

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

/// Nothing to do: Windows decides by extension, and the script is a
/// `.cmd`. The `Result` is here to match the unix one, which can fail.
#[cfg(windows)]
#[allow(clippy::unnecessary_wraps)]
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
