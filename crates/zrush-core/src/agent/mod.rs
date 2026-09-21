//! The port: what zrush needs from a coding agent, and nothing more.
//!
//! zrush routes between git worktrees and the agent conversations attached
//! to them. Which agent, and how it stores those conversations, is an
//! implementation detail behind this trait. Claude Code discovers its
//! sessions through `claude agents --json` and keeps JSONL transcripts under
//! `~/.claude/projects`; another agent may do neither, and only has to
//! answer the questions below.
//!
//! Nothing outside `agent::` may name a concrete agent.

use std::collections::HashSet;
use std::path::Path;

use crate::config::{Config, Dirs};
use crate::error::Result;
use crate::model::{Session, Worktree};

pub mod claude;

/// What the agent is being asked about: this repository, right now.
pub struct Scope<'a> {
    /// The main worktree, which is what the repo is filed under.
    pub main_root: &'a Path,
    /// Where worktrees zrush creates live, so an agent can tell a session
    /// written for a deleted worktree from one written for the repo.
    pub new_root: &'a Path,
    pub worktrees: &'a [Worktree],
    /// Sessions already known to be running: never list them twice.
    pub live_ids: &'a HashSet<String>,
    /// The user asked to see everything, so ignore the scan bound.
    pub uncapped: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

/// One exchange, for the preview pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub role: Role,
    pub text: String,
}

pub trait Agent: Send + Sync {
    /// Stable, written into the worktree's state file. Never translated.
    fn id(&self) -> &'static str;

    /// What the interface calls it.
    fn display_name(&self) -> &'static str;

    /// Is this agent usable on this machine? A missing CLI is not an error,
    /// it just means no rows.
    fn available(&self) -> bool;

    /// Does this string look like one of this agent's session ids? Used
    /// before writing an association, so a malformed value never reaches
    /// the resume command.
    fn valid_id(&self, id: &str) -> bool;

    /// Conversations running right now.
    fn live(&self, dirs: &Dirs, cfg: &Config) -> Vec<Session>;

    /// Conversations that are over but can be resumed, newest first.
    fn resumable(&self, dirs: &Dirs, cfg: &Config, scope: &Scope<'_>) -> Vec<Session>;

    /// How many transcripts exist for a worktree, for its badge. A cheap
    /// count, not a listing.
    fn history_count(&self, dirs: &Dirs, worktree: &Path) -> usize;

    /// The tail of a conversation, for the preview pane.
    fn preview(&self, dirs: &Dirs, id: &str, turns: usize) -> Vec<Turn>;

    /// Delete a conversation for good. Returns how many files went.
    fn delete(&self, dirs: &Dirs, id: &str) -> Result<usize>;

    /// The command line that resumes a conversation.
    fn resume_command(&self, id: &str) -> Vec<String>;

    /// The command line that starts a fresh one.
    fn start_command(&self) -> Vec<String>;
}

/// Every agent this build knows about, whether or not it is installed.
pub fn all() -> Vec<Box<dyn Agent>> {
    vec![Box::new(claude::ClaudeCode)]
}

/// The agents actually usable here.
pub fn available() -> Vec<Box<dyn Agent>> {
    all().into_iter().filter(|a| a.available()).collect()
}

/// The agent an id was recorded against. An unknown or missing name falls
/// back to the first available one, so a state file written before agents
/// were a concept still resumes.
pub fn by_id(agents: &[Box<dyn Agent>], id: Option<&str>) -> Option<usize> {
    match id {
        Some(want) => agents.iter().position(|a| a.id() == want),
        None => (!agents.is_empty()).then_some(0),
    }
}

/// True when the command is on `PATH`.
pub(crate) fn on_path(cmd: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|d| {
        let p = d.join(cmd);
        p.is_file() || p.with_extension("exe").is_file()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_agent_has_a_distinct_id() {
        let ids: Vec<&str> = all().iter().map(|a| a.id()).collect();
        let unique: HashSet<&&str> = ids.iter().collect();
        assert_eq!(ids.len(), unique.len());
    }

    #[test]
    fn a_state_file_with_no_agent_falls_back_to_the_first() {
        let agents = all();
        assert_eq!(by_id(&agents, None), Some(0));
    }

    #[test]
    fn an_unknown_agent_name_matches_nothing() {
        let agents = all();
        assert_eq!(by_id(&agents, Some("no-such-agent")), None);
    }

    #[test]
    fn a_known_agent_name_is_found() {
        let agents = all();
        assert_eq!(by_id(&agents, Some("claude")), Some(0));
    }
}
