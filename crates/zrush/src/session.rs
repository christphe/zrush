//! `zrush session` — the shell an editor runs in its terminal panel.
//!
//! Resumes the conversation bound to THIS worktree, or starts a fresh one,
//! then hands the terminal back to your login shell rather than closing it.
//!
//! It also clears the `CLAUDE_CODE_*` markers an editor may have inherited
//! from the session that launched it. Without that, Claude Code writes no
//! transcript — and a session with no transcript can never be resumed,
//! which is exactly what zrush exists to provide.

use std::path::Path;
use std::process::Command;

use zrush_core::agent::{self, Agent};
use zrush_core::config::{Config, Dirs};
use zrush_core::error::Result;

/// Guards against `zrush session` being someone's `$SHELL` and their login
/// shell invoking it again.
const ACTIVE: &str = "ZRUSH_SESSION_ACTIVE";

pub fn run(dirs: &Dirs, cfg: &Config, pick: bool) -> Result<()> {
    if std::env::var_os(ACTIVE).is_some() {
        login_shell();
        return Ok(());
    }

    let here = std::env::current_dir()?;
    let Ok(toplevel) = zrush_core::git::run(&here, &["rev-parse", "--show-toplevel"]) else {
        login_shell();
        return Ok(());
    };
    let toplevel = Path::new(toplevel.trim());
    if !toplevel.is_dir() {
        login_shell();
        return Ok(());
    }

    let agents = agent::available();
    let binding = zrush_core::state::read(toplevel).ok().flatten();

    // The binding names its own agent; a file written before agents were a
    // concept means the first available one.
    let chosen = binding
        .as_ref()
        .and_then(|b| agent::by_id(&agents, b.agent.as_deref()).map(|i| (i, b.id.clone())));

    let (agent, id) = match chosen {
        Some((i, id)) => (agents.get(i), Some(id)),
        None => (agents.first(), None),
    };
    let Some(agent) = agent else {
        login_shell();
        return Ok(());
    };

    let id = if pick {
        choose(dirs, cfg, agent.as_ref(), toplevel)
    } else {
        id
    };

    let command = match id {
        Some(id) => agent.resume_command(&id),
        None => agent.start_command(),
    };
    let Some((bin, args)) = command.split_first() else {
        login_shell();
        return Ok(());
    };

    // Claude does NOT replace this shell: quitting it lands you in your
    // login shell instead of losing the terminal panel.
    let mut cmd = Command::new(bin);
    cmd.args(args).current_dir(toplevel);
    clean_env(&mut cmd);
    let _ = cmd.status();
    login_shell();
    Ok(())
}

/// Drop the markers on the CHILD's environment rather than on ours.
/// Mutating this process's environment is global, racy, and unsafe in
/// edition 2024 for exactly that reason — and the child is the only thing
/// that needed them gone.
fn clean_env(cmd: &mut Command) {
    cmd.env(ACTIVE, "1");
    if std::env::var_os("ZRUSH_KEEP_CLAUDE_ENV").is_some() {
        return;
    }
    for var in INHERITED {
        cmd.env_remove(var);
    }
}

/// `--list`: choose which conversation to resume rather than taking the
/// bound one.
fn choose(dirs: &Dirs, cfg: &Config, agent: &dyn Agent, worktree: &Path) -> Option<String> {
    let scope = agent::Scope {
        main_root: worktree,
        new_root: worktree,
        worktrees: &[],
        live_ids: &std::collections::HashSet::new(),
        uncapped: false,
    };
    let mut all = agent.live(dirs, cfg);
    all.extend(agent.resumable(dirs, cfg, &scope));
    all.retain(|s| s.cwd.starts_with(worktree));
    if all.is_empty() {
        return None;
    }
    for (i, s) in all.iter().enumerate() {
        eprintln!("  {}) {}  ({})", i + 1, s.title, s.status);
    }
    eprint!("resume which? [enter for a new one] ");
    let mut line = String::new();
    std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut line).ok()?;
    let at: usize = line.trim().parse().ok()?;
    all.get(at.checked_sub(1)?).map(|s| s.id.clone())
}

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

/// Always end in a usable shell. There is no failure worth reporting: if
/// the shell will not start, the terminal is already lost.
fn login_shell() {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let mut cmd = Command::new(shell);
    cmd.arg("-l");
    // The guard travels with it: a login shell that invokes zrush session
    // again must fall straight through instead of looping.
    cmd.env(ACTIVE, "1");
    let _ = cmd.status();
}
