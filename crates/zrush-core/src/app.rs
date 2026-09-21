//! The driving side: everything a front end can ask zrush to do.
//!
//! Both interfaces go through here. The terminal one turns key presses into
//! these calls; an editor extension turns commands or clicks into the same
//! ones. Neither reaches past this type into `git`, `agent` or `state`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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
    /// Which agent the interface is looking at, the way k9s looks at one
    /// context. Listing every agent at once would cost one CLI call and one
    /// transcript scan per agent on every refresh, and produce rows nobody
    /// can tell apart without a column saying which is which.
    ///
    /// It only decides what is *listed*. Opening, resuming and deleting
    /// route through the agent recorded against the session, so a worktree
    /// bound under another agent still resumes correctly.
    /// Interior state: which agent is being looked at changes while the
    /// interface holds the service through an `Arc`, and changing it
    /// restructures nothing.
    active: AtomicUsize,
    /// Several agents are installed and nothing said which to use. The
    /// interface asks before it lists anything.
    must_choose: AtomicBool,
    /// The config named an agent that is not installed here. Not fatal —
    /// a config travels between machines — but never silently swallowed.
    missing_configured: Option<String>,
    host: Box<dyn Host>,
}

impl Zrush {
    /// `requested` is the command line's word on it, and outranks the
    /// config. The order is: what was asked for, then the configured
    /// default, then the only agent installed, then ask.
    pub fn new(
        dirs: Dirs,
        cfg: Config,
        repo: PathBuf,
        agents: Vec<Box<dyn Agent>>,
        host: Box<dyn Host>,
        requested: Option<&str>,
    ) -> Result<Self> {
        // The main worktree is the first porcelain entry, whichever worktree
        // git is run from.
        let main_root = git::worktrees(&repo)?
            .into_iter()
            .next()
            .map(|w| w.path)
            .ok_or_else(|| ZrushError::NotARepo(repo.clone()))?;

        let find = |id: &str| agents.iter().position(|a| a.id() == id);

        // Asked for by name: a typo is an error, not a shrug.
        if let Some(want) = requested {
            let at = find(want).ok_or_else(|| ZrushError::msg(format!("no such agent: {want}")))?;
            return Ok(Self {
                dirs,
                cfg,
                repo,
                main_root,
                agents,
                active: AtomicUsize::new(at),
                must_choose: AtomicBool::new(false),
                missing_configured: None,
                host,
            });
        }

        let (active, must_choose, missing_configured) = match cfg.agent.as_deref() {
            Some(want) => match find(want) {
                Some(at) => (at, false, None),
                // Configured, but not on this machine. Fall back to asking,
                // and let the interface say why.
                None => (0, agents.len() > 1, Some(want.to_string())),
            },
            None => (0, agents.len() > 1, None),
        };

        Ok(Self {
            dirs,
            cfg,
            repo,
            main_root,
            agents,
            active: AtomicUsize::new(active),
            must_choose: AtomicBool::new(must_choose),
            missing_configured,
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

    /// The agent currently being listed. `None` when no agent is installed:
    /// zrush is still a worktree router, it just has no sessions to show.
    pub fn active_agent(&self) -> Option<&dyn Agent> {
        self.agents
            .get(self.active.load(Ordering::SeqCst))
            .map(AsRef::as_ref)
    }

    /// Switch which agent the interface lists. Unknown names are refused
    /// rather than silently ignored: it is typed, in a command bar.
    pub fn set_active_agent(&self, id: &str) -> Result<()> {
        let at = self
            .agents
            .iter()
            .position(|a| a.id() == id)
            .ok_or_else(|| ZrushError::msg(format!("no such agent: {id}")))?;
        self.active.store(at, Ordering::SeqCst);
        self.must_choose.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// True when there is a choice to offer. With one agent installed, the
    /// interface must not ask which one.
    pub fn has_agent_choice(&self) -> bool {
        self.agents.len() > 1
    }

    /// The interface must ask which agent before listing anything.
    pub fn must_choose_agent(&self) -> bool {
        self.must_choose.load(Ordering::SeqCst)
    }

    /// An agent the config asked for that is not installed here, for the
    /// interface to mention once.
    pub fn missing_configured_agent(&self) -> Option<&str> {
        self.missing_configured.as_deref()
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
        with(td, repo, Config::default(), crate::agent::all(), None)
    }

    fn with(
        td: &tempfile::TempDir,
        repo: PathBuf,
        cfg: Config,
        agents: Vec<Box<dyn Agent>>,
        requested: Option<&str>,
    ) -> Zrush {
        Zrush::new(
            crate::config::dirs_under(td.path()),
            cfg,
            repo,
            agents,
            Box::new(RecordingHost::default()),
            requested,
        )
        .unwrap()
    }

    /// A second agent, so the choosing rules have something to choose
    /// between. It is never asked for anything.
    struct Stub;

    impl Agent for Stub {
        fn id(&self) -> &'static str {
            "stub"
        }
        fn display_name(&self) -> &'static str {
            "Stub"
        }
        fn available(&self) -> bool {
            true
        }
        fn valid_id(&self, id: &str) -> bool {
            !id.is_empty()
        }
        fn live(&self, _d: &Dirs, _c: &Config) -> Vec<Session> {
            Vec::new()
        }
        fn resumable(&self, _d: &Dirs, _c: &Config, _s: &Scope<'_>) -> Vec<Session> {
            Vec::new()
        }
        fn history_count(&self, _d: &Dirs, _w: &Path) -> usize {
            0
        }
        fn preview(&self, _d: &Dirs, _i: &str, _t: usize) -> Vec<Turn> {
            Vec::new()
        }
        fn delete(&self, _d: &Dirs, _i: &str) -> Result<usize> {
            Ok(0)
        }
        fn resume_command(&self, id: &str) -> Vec<String> {
            vec!["stub".into(), id.into()]
        }
        fn start_command(&self) -> Vec<String> {
            vec!["stub".into()]
        }
    }

    fn two() -> Vec<Box<dyn Agent>> {
        vec![Box::new(crate::agent::claude::ClaudeCode), Box::new(Stub)]
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
            None,
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

    #[test]
    fn the_first_agent_is_the_one_listed() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch(&td));
        assert_eq!(z.active_agent().map(Agent::id), Some("claude"));
    }

    #[test]
    fn switching_to_an_unknown_agent_is_refused() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch(&td));
        assert!(z.set_active_agent("no-such-agent").is_err());
        assert_eq!(z.active_agent().map(Agent::id), Some("claude"));
    }

    #[test]
    fn one_installed_agent_is_taken_without_asking() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch(&td));
        assert!(!z.must_choose_agent());
        assert_eq!(z.active_agent().map(Agent::id), Some("claude"));
    }

    #[test]
    fn several_installed_and_nothing_configured_means_ask() {
        let td = tempfile::TempDir::new().unwrap();
        let z = with(&td, scratch(&td), Config::default(), two(), None);
        assert!(z.must_choose_agent());
    }

    #[test]
    fn a_configured_default_settles_it() {
        let td = tempfile::TempDir::new().unwrap();
        let cfg = Config {
            agent: Some("stub".into()),
            ..Config::default()
        };
        let z = with(&td, scratch(&td), cfg, two(), None);
        assert!(!z.must_choose_agent());
        assert_eq!(z.active_agent().map(Agent::id), Some("stub"));
    }

    #[test]
    fn the_command_line_outranks_the_configured_default() {
        let td = tempfile::TempDir::new().unwrap();
        let cfg = Config {
            agent: Some("stub".into()),
            ..Config::default()
        };
        let z = with(&td, scratch(&td), cfg, two(), Some("claude"));
        assert_eq!(z.active_agent().map(Agent::id), Some("claude"));
    }

    #[test]
    fn an_agent_asked_for_by_name_that_does_not_exist_is_an_error() {
        let td = tempfile::TempDir::new().unwrap();
        let r = Zrush::new(
            crate::config::dirs_under(td.path()),
            Config::default(),
            scratch(&td),
            two(),
            Box::new(RecordingHost::default()),
            Some("nope"),
        );
        assert!(r.is_err());
    }

    #[test]
    fn a_configured_agent_missing_here_falls_back_to_asking_and_says_so() {
        // A config travels between machines. Neither fatal nor swallowed.
        let td = tempfile::TempDir::new().unwrap();
        let cfg = Config {
            agent: Some("codex".into()),
            ..Config::default()
        };
        let z = with(&td, scratch(&td), cfg, two(), None);
        assert!(z.must_choose_agent());
        assert_eq!(z.missing_configured_agent(), Some("codex"));
    }

    #[test]
    fn with_one_agent_a_stale_configured_name_just_uses_it() {
        let td = tempfile::TempDir::new().unwrap();
        let cfg = Config {
            agent: Some("codex".into()),
            ..Config::default()
        };
        let z = with(&td, scratch(&td), cfg, crate::agent::all(), None);
        assert!(!z.must_choose_agent());
        assert_eq!(z.active_agent().map(Agent::id), Some("claude"));
        assert_eq!(z.missing_configured_agent(), Some("codex"));
    }

    #[test]
    fn choosing_settles_the_question() {
        let td = tempfile::TempDir::new().unwrap();
        let z = with(&td, scratch(&td), Config::default(), two(), None);
        assert!(z.must_choose_agent());
        z.set_active_agent("stub").unwrap();
        assert!(!z.must_choose_agent());
    }

    #[test]
    fn one_installed_agent_means_nothing_to_choose_between() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch(&td));
        assert!(!z.has_agent_choice());
    }

    #[test]
    fn with_no_agent_at_all_it_is_still_a_worktree_router() {
        let td = tempfile::TempDir::new().unwrap();
        let repo = scratch(&td);
        let z = Zrush::new(
            crate::config::dirs_under(td.path()),
            Config::default(),
            repo,
            Vec::new(),
            Box::new(RecordingHost::default()),
            None,
        )
        .unwrap();
        assert!(z.active_agent().is_none());
        assert!(z.live_sessions().is_empty());
        assert_eq!(z.worktrees().unwrap().len(), 1);
    }

    /// An association written under another agent must survive the active
    /// one being different: opening routes on the binding, not on the view.
    #[test]
    fn a_binding_records_the_agent_it_was_made_with() {
        let td = tempfile::TempDir::new().unwrap();
        let z = zrush(&td, scratch(&td));
        let dest = z.create_worktree("feature").unwrap();
        let id = "aaaaaaaa-1111-2222-3333-444455556666";
        z.open(&dest, Some(("claude", id))).unwrap();
        assert_eq!(z.binding(&dest).unwrap().agent.as_deref(), Some("claude"));
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
