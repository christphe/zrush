//! The `PATH` a terminal would have, for a window started from the Dock.
//!
//! macOS starts an app from the Finder or the Dock with launchd's `PATH`,
//! `/usr/bin:/bin:/usr/sbin:/sbin`. `claude`, the editor's command and a
//! Homebrew `git` live elsewhere, so the window would list no live session
//! and open nothing. The login shell knows the real `PATH`: the window asks
//! it once, and the core starts every command with the answer.

use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Brackets the answer: a shell's startup files may print anything.
const MARK: &str = "__zrush_path__";

/// A shell whose startup files hang must not keep the window from opening.
const WAIT: Duration = Duration::from_secs(5);

/// Outside a terminal, ask the login shell for its `PATH` and have the core
/// use it. From a terminal the `PATH` is already that shell's.
#[cfg(target_os = "macos")]
pub fn adopt() {
    use std::io::IsTerminal;

    if std::io::stdin().is_terminal() {
        return;
    }
    let shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/zsh".into());
    if let Some(path) = ask(Path::new(&shell)) {
        zrush_core::search_path::adopt(path);
    }
}

#[cfg(not(target_os = "macos"))]
pub fn adopt() {}

/// The `PATH` an interactive login shell ends up with, or `None` when the
/// shell failed, said nothing usable, or took too long.
pub fn ask(shell: &Path) -> Option<OsString> {
    let script = format!("printf '%s%s%s' '{MARK}' \"$PATH\" '{MARK}'");
    let mut child = Command::new(shell)
        .args(["-ilc", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < WAIT => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let out = child.wait_with_output().ok()?;
    parse(&String::from_utf8_lossy(&out.stdout)).map(OsString::from)
}

/// What sits between the two marks, if anything.
fn parse(out: &str) -> Option<&str> {
    let start = out.find(MARK)? + MARK.len();
    let rest = &out[start..];
    let path = &rest[..rest.find(MARK)?];
    (!path.trim().is_empty()).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_is_what_sits_between_the_marks() {
        let out = format!("Welcome!\n{MARK}/opt/homebrew/bin:/usr/bin{MARK}");
        assert_eq!(parse(&out), Some("/opt/homebrew/bin:/usr/bin"));
    }

    #[test]
    fn no_marks_or_nothing_between_them_is_no_answer() {
        assert_eq!(parse("command not found"), None);
        assert_eq!(parse(&format!("{MARK}{MARK}")), None);
        assert_eq!(parse(&format!("{MARK}/usr/bin")), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_shell_is_asked_for_its_path() {
        let path = ask(Path::new("/bin/sh")).expect("sh answers");
        assert!(!path.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_shell_that_cannot_run_is_no_answer() {
        assert_eq!(ask(Path::new("/nonexistent/shell")), None);
    }
}
