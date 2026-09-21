//! The port for everything zrush wants done *to the outside world* but has
//! no business deciding how.
//!
//! Opening a worktree means `zed -n <path>` in the terminal interface, and a
//! call into the editor's own API inside an extension. Starting an agent
//! means a new terminal window in one and a terminal panel in the other.
//! The core states the intent; the host carries it out.
//!
//! This is what makes a second front end possible: a VS Code extension
//! supplies its own `Host` and never spawns an editor.

use std::path::Path;

use crate::error::Result;

pub trait Host: Send + Sync {
    /// Show this worktree to the user, in whatever this host calls an
    /// editor. It is not required to be a new window, and not required to
    /// be a process.
    fn open_workspace(&self, path: &Path) -> Result<()>;

    /// Run an agent command with `cwd` as its working directory, somewhere
    /// the user can type into it.
    ///
    /// `command` comes from `Agent::resume_command` or
    /// `Agent::start_command`; the host decides where it runs.
    fn run_agent(&self, cwd: &Path, command: &[String]) -> Result<()>;
}

/// A host that refuses everything, for tests and for `--print`, where no
/// side effect is wanted.
pub struct NullHost;

impl Host for NullHost {
    fn open_workspace(&self, _path: &Path) -> Result<()> {
        Ok(())
    }

    fn run_agent(&self, _cwd: &Path, _command: &[String]) -> Result<()> {
        Ok(())
    }
}

/// A host that records what it was asked for, so a test can assert on the
/// decision rather than on a process having been spawned.
#[derive(Default)]
pub struct RecordingHost {
    pub opened: std::sync::Mutex<Vec<std::path::PathBuf>>,
    pub ran: std::sync::Mutex<Vec<(std::path::PathBuf, Vec<String>)>>,
}

impl Host for RecordingHost {
    fn open_workspace(&self, path: &Path) -> Result<()> {
        if let Ok(mut v) = self.opened.lock() {
            v.push(path.to_path_buf());
        }
        Ok(())
    }

    fn run_agent(&self, cwd: &Path, command: &[String]) -> Result<()> {
        if let Ok(mut v) = self.ran.lock() {
            v.push((cwd.to_path_buf(), command.to_vec()));
        }
        Ok(())
    }
}
