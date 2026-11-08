//! What each action does, and the line it reports, whatever interface
//! asked for it. An interface decides when to run one and how to show the
//! answer — a spinner and a status line, a toast — never what it does.

use std::path::{Path, PathBuf};

use crate::app::{BranchKind, Zrush};
use crate::config::Config;
use crate::error::{Result, ZrushError};
use crate::model::Session;

/// File a conversation under `worktree` and open it there, bound to it.
pub fn adopt(z: &Zrush, agent: &str, id: &str, worktree: &Path) -> Result<String> {
    z.adopt_session(agent, id, worktree)?;
    // Some, not None: None means "a fresh conversation here" and clears the
    // association adopt_session just wrote, which is the whole point of
    // adopting it.
    z.open(worktree, Some((agent, id)))?;
    Ok(format!("moved to {}", worktree.display()))
}

pub fn delete_session(z: &Zrush, agent: &str, id: &str, worktree: Option<&Path>) -> Result<String> {
    let n = z.delete_session(agent, id, false, worktree)?;
    Ok(format!("deleted {n} file(s)"))
}

/// A new worktree, opened at once. `hand_over` is a conversation to bind
/// to it, which is how a session moves to a worktree of its own.
pub fn create(
    z: &Zrush,
    name: &str,
    kind: BranchKind,
    at: Option<&Path>,
    hand_over: Option<(&str, &str)>,
) -> Result<String> {
    let dest = z.create_worktree_at(at, name, kind)?;
    z.open(&dest, hand_over)?;
    Ok(format!("created {name}"))
}

/// `sessions` are the ones held in `path`: deleted with it when
/// `with_sessions`, running ones always kept.
pub fn remove(z: &Zrush, path: &Path, sessions: &[Session], with_sessions: bool) -> Result<String> {
    let r = z.remove_worktree(path, sessions, with_sessions)?;
    Ok(format!(
        "removed, {} transcript(s) deleted, {} running kept",
        r.transcripts, r.running_kept
    ))
}

/// Each item is independent: a dirty worktree refuses without stopping the
/// rest, and only the closing summary is reported.
pub fn purge(z: &Zrush, worktrees: &[PathBuf], sessions: &[(String, String)]) -> Result<String> {
    let mut removed = 0;
    let mut refused = 0;
    for p in worktrees {
        if z.remove_worktree(p, &[], false).is_ok() {
            removed += 1;
        } else {
            refused += 1;
        }
    }
    let gone: usize = sessions
        .iter()
        .map(|(a, i)| z.delete_session(a, i, false, None).unwrap_or(0))
        .sum();
    let kept = if refused > 0 {
        format!(", {refused} kept: git refused")
    } else {
        String::new()
    };
    Ok(format!(
        "purged {removed} worktree(s), {gone} transcript(s){kept}"
    ))
}

/// Start an agent in a terminal of its own. The session's own agent
/// outranks the one being listed: a worktree bound under another agent
/// still resumes correctly.
pub fn run_agent(z: &Zrush, worktree: &Path, session: Option<(&str, &str)>) -> Result<()> {
    let listed = z.active_agent().map(crate::agent::Agent::id);
    let agent = session
        .map(|(a, _)| a)
        .or(listed)
        .ok_or(ZrushError::NoAgent)?;
    z.run_agent(worktree, agent, session.map(|(_, i)| i))
}

/// List another agent, and start on it next time too.
pub fn switch_agent(z: &Zrush, id: &str) -> Result<Config> {
    z.set_active_agent(id)?;
    z.edit_config(|c| c.agent = Some(id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, dirs_under};
    use crate::host::RecordingHost;
    use crate::testing::{git, scratch_repo, zrush};

    #[test]
    fn a_purge_says_how_many_git_refused() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                ".claude/worktrees/clean",
                "-b",
                "clean",
            ],
        );
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                ".claude/worktrees/dirty",
                "-b",
                "dirty",
            ],
        );
        let clean = repo.join(".claude/worktrees/clean");
        let dirty = repo.join(".claude/worktrees/dirty");
        std::fs::write(dirty.join("untracked.txt"), "x").unwrap();
        let z = zrush(&td, repo);
        assert_eq!(
            purge(&z, &[clean, dirty], &[]).unwrap(),
            "purged 1 worktree(s), 0 transcript(s), 1 kept: git refused"
        );
    }

    #[test]
    fn a_clean_purge_says_nothing_was_kept() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        git(
            &repo,
            &["worktree", "add", "-q", ".claude/worktrees/a", "-b", "a"],
        );
        let z = zrush(&td, repo.clone());
        assert_eq!(
            purge(&z, &[repo.join(".claude/worktrees/a")], &[]).unwrap(),
            "purged 1 worktree(s), 0 transcript(s)"
        );
    }

    #[test]
    fn creating_a_worktree_says_its_name() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch_repo(&td));
        assert_eq!(
            create(&z, "feature", crate::app::BranchKind::New, None, None).unwrap(),
            "created feature"
        );
    }

    #[test]
    fn removing_a_worktree_counts_what_went() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        git(
            &repo,
            &["worktree", "add", "-q", ".claude/worktrees/a", "-b", "a"],
        );
        let z = zrush(&td, repo.clone());
        assert_eq!(
            remove(&z, &repo.join(".claude/worktrees/a"), &[], false).unwrap(),
            "removed, 0 transcript(s) deleted, 0 running kept"
        );
    }

    #[test]
    fn run_agent_with_no_agent_at_all_says_so() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch_repo(&td);
        let z = crate::app::Zrush::new(
            dirs_under(td.path()),
            Config::default(),
            repo.clone(),
            Vec::new(),
            Box::new(RecordingHost::default()),
            None,
        )
        .unwrap();
        let e = run_agent(&z, &repo, None).unwrap_err();
        assert!(matches!(e, crate::error::ZrushError::NoAgent));
        assert_eq!(e.to_string(), "no agent installed");
    }

    #[test]
    fn switching_agent_is_remembered() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch_repo(&td));
        let cfg = switch_agent(&z, "claude").unwrap();
        assert_eq!(cfg.agent.as_deref(), Some("claude"));
        assert_eq!(
            Config::load_file(&dirs_under(td.path()))
                .unwrap()
                .agent
                .as_deref(),
            Some("claude")
        );
    }

    #[test]
    fn switching_to_an_unknown_agent_changes_nothing() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch_repo(&td));
        assert!(switch_agent(&z, "nope").is_err());
        assert!(!dirs_under(td.path()).config_file().exists());
    }
}
