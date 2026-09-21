//! The driving side: everything a front end can ask zrush to do.
//!
//! Both interfaces go through here. The terminal one turns key presses into
//! these calls; an editor extension turns commands or clicks into the same
//! ones. Neither reaches past this type into `git`, `agent` or `state`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::agent::{Agent, Scope, Turn};
use crate::config::{Config, Dirs};
use crate::error::{Result, ZrushError};
use crate::host::Host;
use crate::model::{GitStatus, Session, Worktree};
use crate::{git, state};

/// What was removed, so a front end can say so in its own words.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Removed {
    pub transcripts: usize,
    pub running_kept: usize,
}

pub struct Zrush {
    dirs: Dirs,
    cfg: Config,
    /// Any worktree of the repository; git reports the whole set from any.
    repo: PathBuf,
    main_root: PathBuf,
    agents: Vec<Box<dyn Agent>>,
    host: Box<dyn Host>,
}

impl Zrush {
    pub fn new(
        dirs: Dirs,
        cfg: Config,
        repo: PathBuf,
        agents: Vec<Box<dyn Agent>>,
        host: Box<dyn Host>,
    ) -> Result<Self> {
        // The main worktree is the first porcelain entry, whichever worktree
        // git is run from.
        let main_root = git::worktrees(&repo)?
            .into_iter()
            .next()
            .map(|w| w.path)
            .ok_or_else(|| ZrushError::NotARepo(repo.clone()))?;
        Ok(Self {
            dirs,
            cfg,
            repo,
            main_root,
            agents,
            host,
        })
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    pub fn main_root(&self) -> &Path {
        &self.main_root
    }

    /// New worktrees land beside the ones `claude --worktree` makes.
    pub fn new_root(&self) -> PathBuf {
        self.main_root.join(".claude").join("worktrees")
    }

    pub fn agents(&self) -> &[Box<dyn Agent>] {
        &self.agents
    }

    fn agent(&self, id: &str) -> Result<&dyn Agent> {
        self.agents
            .iter()
            .find(|a| a.id() == id)
            .map(AsRef::as_ref)
            .ok_or_else(|| ZrushError::msg(format!("no such agent: {id}")))
    }

    // ------------------------------------------------------------ read ---

    pub fn worktrees(&self) -> Result<Vec<Worktree>> {
        git::worktrees(&self.repo)
    }

    pub fn status(&self, worktree: &Path) -> Result<GitStatus> {
        git::status(worktree)
    }

    /// Conversations running right now, across every available agent.
    pub fn live_sessions(&self) -> Vec<Session> {
        self.agents
            .iter()
            .flat_map(|a| a.live(&self.dirs, &self.cfg))
            .collect()
    }

    /// Conversations that are over but can be resumed. `uncapped` lifts the
    /// scan bound, which the interface does once the user asks to see past
    /// the first few.
    pub fn resumable_sessions(
        &self,
        worktrees: &[Worktree],
        live: &[Session],
        uncapped: bool,
    ) -> Vec<Session> {
        let live_ids: HashSet<String> = live.iter().map(|s| s.id.clone()).collect();
        let new_root = self.new_root();
        let scope = Scope {
            main_root: &self.main_root,
            new_root: &new_root,
            worktrees,
            live_ids: &live_ids,
            uncapped,
        };
        self.agents
            .iter()
            .flat_map(|a| a.resumable(&self.dirs, &self.cfg, &scope))
            .collect()
    }

    /// How many past conversations a worktree has, summed over the agents.
    pub fn history_count(&self, worktree: &Path) -> usize {
        self.agents
            .iter()
            .map(|a| a.history_count(&self.dirs, worktree))
            .sum()
    }

    pub fn preview(&self, agent: &str, id: &str) -> Vec<Turn> {
        self.agent(agent)
            .map(|a| a.preview(&self.dirs, id, self.cfg.preview_turns))
            .unwrap_or_default()
    }

    pub fn binding(&self, worktree: &Path) -> Option<state::Binding> {
        state::read(worktree).ok().flatten()
    }

    // ----------------------------------------------------------- write ---

    /// Open a worktree, binding it to a session first or clearing whatever
    /// it was bound to. A worktree always means a fresh conversation; a
    /// session means resume that one.
    pub fn open(&self, worktree: &Path, bind: Option<(&str, &str)>) -> Result<()> {
        match bind {
            Some((agent, id)) => {
                if !self.agent(agent)?.valid_id(id) {
                    return Err(ZrushError::msg("refusing to store a malformed session id"));
                }
                state::write(worktree, agent, id)?;
            }
            None => state::clear(worktree)?,
        }
        self.host.open_workspace(worktree)
    }

    /// Start an agent for this worktree without touching the association or
    /// the editor.
    pub fn run_agent(&self, worktree: &Path, agent: &str, resume: Option<&str>) -> Result<()> {
        let a = self.agent(agent)?;
        let command = resume.map_or_else(|| a.start_command(), |id| a.resume_command(id));
        self.host.run_agent(worktree, &command)
    }

    /// Add a worktree under `new_root`. An existing branch is checked out
    /// as-is; a new one is cut from the default base.
    pub fn create_worktree(&self, name: &str) -> Result<PathBuf> {
        let name = name.trim();
        if !git::check_ref_format(name) {
            return Err(ZrushError::msg(format!("invalid branch name: {name}")));
        }
        let new_root = self.new_root();
        // Only the last segment: a branch called feat/thing gets a directory
        // called thing, not a nested pair.
        let leaf = name.rsplit('/').next().unwrap_or(name);
        let dest = new_root.join(leaf);
        if dest.exists() {
            return Err(ZrushError::msg(format!(
                "already there: {}",
                dest.display()
            )));
        }
        std::fs::create_dir_all(&new_root)?;

        let from = if git::branch_exists(&self.repo, name) {
            git::AddFrom::ExistingBranch(name.to_string())
        } else {
            git::AddFrom::NewBranch {
                name: name.to_string(),
                base: git::default_base(&self.repo)?,
            }
        };
        git::worktree_add(&self.repo, &dest, &from)?;
        Ok(dest)
    }

    /// Delete one conversation for good. A running one is refused: stop it
    /// first. If the worktree pointed at it, the association goes too.
    pub fn delete_session(
        &self,
        agent: &str,
        id: &str,
        running: bool,
        bound_to: Option<&Path>,
    ) -> Result<usize> {
        if running {
            return Err(ZrushError::msg(format!(
                "session {id} is running; stop it first"
            )));
        }
        let gone = self.agent(agent)?.delete(&self.dirs, id)?;
        if gone == 0 {
            return Err(ZrushError::msg(format!("no transcript found for {id}")));
        }
        if let Some(p) = bound_to {
            if self.binding(p).is_some_and(|b| b.id == id) {
                state::clear(p)?;
            }
        }
        Ok(gone)
    }

    /// Remove a worktree and, when asked, the conversations that belong to
    /// it. Two decisions, not one: removing a worktree and deleting the
    /// conversations that happened in it are not the same thing.
    pub fn remove_worktree(
        &self,
        path: &Path,
        sessions: &[Session],
        with_sessions: bool,
    ) -> Result<Removed> {
        if path == self.main_root {
            return Err(ZrushError::msg("refusing to remove the main worktree"));
        }
        git::worktree_remove(&self.repo, path)?;
        if !with_sessions {
            return Ok(Removed::default());
        }
        let mut out = Removed::default();
        for s in sessions {
            if s.kind == crate::model::SessionKind::Live {
                out.running_kept += 1;
                continue;
            }
            if let Ok(a) = self.agent(s.agent) {
                out.transcripts += a.delete(&self.dirs, &s.id).unwrap_or(0);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::RecordingHost;

    /// A repo with one commit on `main`, cloned from a bare remote so
    /// `origin/main` exists. Nothing here touches a real repository.
    fn scratch(td: &tempfile::TempDir) -> PathBuf {
        let base = td.path().canonicalize().unwrap();
        let remote = base.join("remote.git");
        let repo = base.join("repo");
        run(&base, &["init", "-q", "--bare", remote.to_str().unwrap()]);
        run(
            &base,
            &[
                "clone",
                "-q",
                remote.to_str().unwrap(),
                repo.to_str().unwrap(),
            ],
        );
        run(&repo, &["config", "user.email", "t@example.invalid"]);
        run(&repo, &["config", "user.name", "zrush tests"]);
        std::fs::write(repo.join("f.txt"), "seed").unwrap();
        run(&repo, &["add", "f.txt"]);
        run(&repo, &["commit", "-qm", "init"]);
        run(&repo, &["branch", "-M", "main"]);
        run(&repo, &["push", "-q", "-u", "origin", "main"]);
        repo
    }

    fn run(cwd: &Path, args: &[&str]) {
        let st = std::process::Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?} failed");
    }

    fn zrush(td: &tempfile::TempDir, repo: PathBuf) -> Zrush {
        Zrush::new(
            crate::config::dirs_under(td.path()),
            Config::default(),
            repo,
            crate::agent::all(),
            Box::new(RecordingHost::default()),
        )
        .unwrap()
    }

    #[test]
    fn the_main_worktree_is_found_from_any_worktree() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch(&td);
        let z = zrush(&td, repo.clone());
        assert_eq!(z.main_root(), repo);
    }

    #[test]
    fn creating_a_worktree_cuts_the_branch_and_lands_under_new_root() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch(&td);
        let z = zrush(&td, repo.clone());
        let dest = z.create_worktree("feature").unwrap();
        assert_eq!(dest, repo.join(".claude/worktrees/feature"));
        assert!(dest.join("f.txt").exists());
    }

    #[test]
    fn a_slashed_branch_gets_a_flat_directory() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch(&td));
        let dest = z.create_worktree("feat/thing").unwrap();
        assert!(dest.ends_with("thing"));
    }

    #[test]
    fn an_invalid_branch_name_is_refused_before_git_is_asked() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch(&td));
        assert!(z.create_worktree("has space").is_err());
    }

    #[test]
    fn creating_the_same_worktree_twice_is_refused() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch(&td));
        z.create_worktree("feature").unwrap();
        assert!(z.create_worktree("feature").is_err());
    }

    #[test]
    fn opening_a_worktree_clears_the_association_and_tells_the_host() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch(&td);
        let host = std::sync::Arc::new(RecordingHost::default());
        let z = Zrush::new(
            crate::config::dirs_under(td.path()),
            Config::default(),
            repo.clone(),
            crate::agent::all(),
            Box::new(ArcHost(host.clone())),
        )
        .unwrap();
        let dest = z.create_worktree("feature").unwrap();
        state::write(&dest, "claude", "aaaaaaaa-1111-2222-3333-444455556666").unwrap();

        z.open(&dest, None).unwrap();

        assert!(z.binding(&dest).is_none());
        assert_eq!(host.opened.lock().unwrap().as_slice(), &[dest]);
    }

    #[test]
    fn opening_a_session_binds_it_first() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch(&td));
        let dest = z.create_worktree("feature").unwrap();
        let id = "aaaaaaaa-1111-2222-3333-444455556666";
        z.open(&dest, Some(("claude", id))).unwrap();
        let b = z.binding(&dest).unwrap();
        assert_eq!(b.id, id);
        assert_eq!(b.agent.as_deref(), Some("claude"));
    }

    #[test]
    fn a_malformed_id_never_reaches_the_state_file() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch(&td));
        let dest = z.create_worktree("feature").unwrap();
        assert!(z.open(&dest, Some(("claude", "not-a-uuid"))).is_err());
        assert!(z.binding(&dest).is_none());
    }

    #[test]
    fn the_main_worktree_is_never_removed() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch(&td);
        let z = zrush(&td, repo.clone());
        assert!(z.remove_worktree(&repo, &[], false).is_err());
        assert!(repo.exists());
    }

    #[test]
    fn a_running_session_is_never_deleted() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch(&td));
        assert!(z.delete_session("claude", "any", true, None).is_err());
    }

    #[test]
    fn an_unknown_agent_is_an_error_not_a_panic() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch(&td));
        assert!(z.delete_session("codex", "any", false, None).is_err());
    }

    /// Lets a test keep a handle on the host the service owns.
    struct ArcHost(std::sync::Arc<RecordingHost>);

    impl crate::host::Host for ArcHost {
        fn open_workspace(&self, path: &Path) -> Result<()> {
            self.0.open_workspace(path)
        }

        fn run_agent(&self, cwd: &Path, command: &[String]) -> Result<()> {
            self.0.run_agent(cwd, command)
        }
    }
}
