//! The `PATH` every command zrush runs is looked up in.
//!
//! A terminal hands zrush the `PATH` its shell built. A window started from
//! the Dock gets launchd's, `/usr/bin:/bin:/usr/sbin:/sbin`, where neither
//! `claude` nor most editors' commands are. The window asks the login shell
//! once and adopts its answer here; every command is then started with it,
//! and looked up in it.

use std::ffi::{OsStr, OsString};
use std::process::Command;
use std::sync::OnceLock;

static ADOPTED: OnceLock<OsString> = OnceLock::new();

/// Use this `PATH` from now on. Only the first call counts: commands
/// already running keep what they started with, so it is set once, early.
pub fn adopt(path: OsString) -> bool {
    ADOPTED.set(path).is_ok()
}

/// The `PATH` commands are looked up in: the adopted one, or the process's.
pub fn get() -> Option<OsString> {
    ADOPTED.get().cloned().or_else(|| std::env::var_os("PATH"))
}

/// A command looked up in, and started with, the adopted `PATH`, that
/// works out of sight: git, the agent's listing, an editor's launcher.
///
/// On Windows a console program started by a window that has no console
/// gets one of its own, an empty black window for as long as it runs:
/// every git call of a refresh would flash one up.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let mut c = command_in(program, ADOPTED.get().map(OsString::as_os_str));
    hide_console(&mut c);
    c
}

/// The same, for a program meant to be seen: the terminal an agent runs
/// in keeps its window.
pub fn visible(program: impl AsRef<OsStr>) -> Command {
    command_in(program, ADOPTED.get().map(OsString::as_os_str))
}

#[cfg(windows)]
fn hide_console(c: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    c.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_: &mut Command) {}

/// Setting `PATH` on the command is enough: on Unix, a `PATH` given to the
/// child is also where the program is looked for.
fn command_in(program: impl AsRef<OsStr>, path: Option<&OsStr>) -> Command {
    let mut c = Command::new(program);
    if let Some(path) = path {
        c.env("PATH", path);
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_program_is_found_in_the_path_it_was_given() {
        use std::os::unix::fs::PermissionsExt;
        let td = tempfile::TempDir::new().unwrap();
        let bin = td.path().join("zrush-test-hello");
        std::fs::write(&bin, "#!/bin/sh\necho \"$PATH\"\n").unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = std::env::join_paths([td.path(), std::path::Path::new("/bin")]).unwrap();

        let out = command_in("zrush-test-hello", Some(&path))
            .output()
            .unwrap();
        assert!(out.status.success());
        // And the program itself runs with it, for what it starts in turn.
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            path.to_string_lossy()
        );
    }

    #[test]
    fn without_an_adopted_path_nothing_is_set() {
        let c = command_in("git", None);
        assert!(c.get_envs().next().is_none());
    }
}
