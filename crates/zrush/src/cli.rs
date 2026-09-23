//! The command line, and what each invocation does.

use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use zrush_core::agent;
use zrush_core::app::Zrush;
use zrush_core::config::{Config, Dirs};
use zrush_core::error::{Result, ZrushError};
use zrush_core::host::{Host, NullHost};

use crate::host::ProcessHost;
use crate::ui::app::App;

#[derive(Parser, Debug)]
#[command(
    name = "zrush",
    about = "Pick a git worktree, or one of its agent sessions, and open it",
    version
)]
pub struct Cli {
    /// Use this repo instead of working it out from the current directory.
    #[arg(short = 'C', long = "repo", value_name = "PATH")]
    pub repo: Option<PathBuf>,

    /// Which agent to list. Outranks the configured default.
    #[arg(short = 'a', long = "agent", value_name = "ID")]
    pub agent: Option<String>,

    /// Print the rows and exit, without drawing anything.
    #[arg(short = 'l', long = "list")]
    pub list: bool,

    /// Skip the session lookup entirely (faster).
    #[arg(short = 'n', long = "no-sessions")]
    pub no_sessions: bool,

    /// Skip the git status column (faster on huge repos).
    #[arg(short = 'S', long = "no-status")]
    pub no_status: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Resume this worktree's session, or start a new one. This is the
    /// shell an editor runs in its terminal panel.
    Session {
        /// Pick which of this worktree's sessions to resume, instead of
        /// taking the one zrush bound to it.
        #[arg(short = 'l', long = "list")]
        list: bool,
    },
    /// Open this worktree's conversation the way this editor does it: the
    /// Claude tab in VS Code, a terminal in Zed. Meant to be bound to a key
    /// by the editor, which is the one asking.
    Tab {
        /// Which editor is asking, when it is not the configured one.
        #[arg(short = 'e', long = "editor", value_name = "NAME")]
        editor: Option<String>,
        /// Say what would be opened instead of opening it.
        #[arg(long = "print")]
        print: bool,
    },
    /// One line per session of a worktree, for `session --list`.
    Sessions { path: PathBuf },
}

/// Where the repo comes from: the flag, then the current directory, then
/// the configured default.
fn locate(cli: &Cli, cfg: &Config) -> Result<PathBuf> {
    if let Some(p) = &cli.repo {
        if !p.is_dir() {
            return Err(ZrushError::msg(format!("not a directory: {}", p.display())));
        }
        return Ok(p.clone());
    }
    let here = std::env::current_dir()?;
    if zrush_core::git::absolute_git_dir(&here).is_ok() {
        return Ok(here);
    }
    match &cfg.default_repo {
        Some(p) if p.is_dir() => Ok(p.clone()),
        _ => Err(ZrushError::msg(
            "not inside a git repository. Use --repo <path>, or set default_repo in the config",
        )),
    }
}

pub fn main() -> Result<()> {
    let cli = Cli::parse();
    let dirs = Dirs::from_env()?;
    let mut cfg = Config::load(&dirs)?;
    if cli.no_status {
        cfg.status = false;
    }

    match &cli.command {
        Some(Command::Session { list }) => return crate::session::run(&dirs, &cfg, *list),
        Some(Command::Tab { editor, print }) => return tab(&cfg, &dirs, editor.as_deref(), *print),
        Some(Command::Sessions { path }) => {
            let z = build(&cli, &cfg, &dirs, path.clone(), Box::new(NullHost))?;
            let live = z.live_sessions();
            for s in &live {
                println!("{}\t● {}  ({})", s.id, s.title, s.status);
            }
            let wts = z.worktrees()?;
            for s in z.resumable_sessions(&wts, &live, false) {
                println!("{}\t◌ {}  ({})", s.id, s.title, s.status);
            }
            return Ok(());
        }
        None => {}
    }

    let repo = locate(&cli, &cfg)?;
    let host: Box<dyn Host> = if cli.list {
        Box::new(NullHost)
    } else {
        Box::new(ProcessHost::new(&cfg))
    };
    let z = Arc::new(build(&cli, &cfg, &dirs, repo, host)?);

    // Derived data only, safe to drop: keep a month.
    zrush_core::agent::claude::cache::sweep(&dirs, 30);

    if cli.list {
        return list(&z);
    }

    let mut app = App::new(cfg.more_step, cfg.resumable_max);
    app.cfg = cfg.clone();
    app.agent_names = z.agents().iter().map(|a| a.id().to_string()).collect();
    app.active_agent = z
        .active_agent()
        .map(zrush_core::agent::Agent::id)
        .unwrap_or_default()
        .to_string();
    app.editor = z.editor_label();
    if z.must_choose_agent() {
        let reason = z
            .missing_configured_agent()
            .map(|a| format!("{a} is configured, but not installed here"));
        app.open_agent_picker(reason);
    } else if let Some(a) = z.missing_configured_agent() {
        app.flash(format!("{a} is configured, but not installed here"));
    }
    crate::ui::screen::run(app, &z)
}

fn build(
    cli: &Cli,
    cfg: &Config,
    dirs: &Dirs,
    repo: PathBuf,
    host: Box<dyn Host>,
) -> Result<Zrush> {
    // -n means: do not go looking for sessions at all. With no agent to
    // list, naming one has nothing left to select, so -a is dropped rather
    // than failing to match an empty list.
    if cli.no_sessions {
        return Zrush::new(dirs.clone(), cfg.clone(), repo, Vec::new(), host, None);
    }
    Zrush::new(
        dirs.clone(),
        cfg.clone(),
        repo,
        agent::available(),
        host,
        cli.agent.as_deref(),
    )
}

/// `zrush tab`: the conversation bound to the worktree we are standing in,
/// opened through whatever surface this editor and that agent have.
///
/// The editor is the caller, not a guess: a task in VS Code says so, and
/// the config answers otherwise. Nothing here knows what a Claude tab is —
/// that is a row in the config.
fn tab(cfg: &Config, dirs: &Dirs, editor: Option<&str>, print: bool) -> Result<()> {
    let mut cfg = cfg.clone();
    if let Some(e) = editor {
        cfg.editor = e.to_string();
    }
    let here = std::env::current_dir()?;
    let toplevel = zrush_core::git::run(&here, &["rev-parse", "--show-toplevel"])
        .map_err(|_| ZrushError::msg("not inside a git worktree"))?;
    let toplevel = PathBuf::from(toplevel.trim());

    let binding = zrush_core::state::read(&toplevel)?;
    let agents = agent::available();
    // The binding names its own agent; with none, whatever is installed.
    let agent = binding
        .as_ref()
        .and_then(|b| b.agent.clone())
        .or_else(|| agents.first().map(|a| a.id().to_string()))
        .ok_or_else(|| ZrushError::msg("no agent installed"))?;
    let session = binding.as_ref().map(|b| b.id.as_str());

    if print {
        let surface = cfg.surface_for(&agent);
        let a = agents
            .iter()
            .find(|a| a.id() == agent)
            .ok_or_else(|| ZrushError::msg(format!("no such agent: {agent}")))?;
        println!(
            "{}\t{}",
            surface.id(),
            surface.describe(&zrush_core::surface::SessionRef {
                worktree: &toplevel,
                agent: a.as_ref(),
                session_id: session,
            })
        );
        return Ok(());
    }

    let host: Box<dyn Host> = Box::new(ProcessHost::new(&cfg));
    let z = Zrush::new(dirs.clone(), cfg, toplevel.clone(), agents, host, None)?;
    z.open_session(&toplevel, &agent, session)
}

/// `--list`: the rows as plain text, for a script or a quick look.
fn list(z: &Zrush) -> Result<()> {
    let worktrees = z.worktrees()?;
    let live = z.live_sessions();
    let resumable = z.resumable_sessions(&worktrees, &live, false);
    let mut sessions = live.clone();
    sessions.extend(resumable);

    let new_root = z.new_root();
    let assigned = zrush_core::model::tree::assign(&sessions, &worktrees, z.main_root(), &new_root);
    let history = worktrees
        .iter()
        .map(|w| (w.path.clone(), z.history_count(&w.path)))
        .collect();
    let statuses = worktrees
        .iter()
        .filter_map(|w| z.status(&w.path).ok().map(|s| (w.path.clone(), s)))
        .collect();

    let rows = zrush_core::model::tree::build(&zrush_core::model::tree::TreeInput {
        worktrees: &worktrees,
        assigned: &assigned,
        statuses: &statuses,
        history: &history,
        collapsed: &std::collections::HashSet::new(),
        expanded: &std::collections::HashMap::new(),
        resumable_max: z.config().resumable_max,
        show_all: false,
    });
    for r in rows {
        println!(
            "{}{}\t{}\t{}\t{}",
            r.glyph, r.label, r.badge, r.status, r.location
        );
    }
    Ok(())
}
