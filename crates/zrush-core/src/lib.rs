//! zrush, without an interface.
//!
//! This crate routes between git worktrees and the coding-agent
//! conversations attached to them. It draws nothing and prints nothing, so
//! the terminal interface and an editor extension can both drive it.
//!
//! Two ports face outwards:
//!
//! - [`agent::Agent`] — how a coding agent's sessions are discovered,
//!   previewed, resumed and deleted. Claude Code is one implementation.
//! - [`host::Host`] — how a worktree gets shown to the user and where an
//!   agent runs. A terminal spawns processes; an extension calls its editor.
//!
//! The association worktree <-> session lives in the worktree's own gitdir,
//! at `<git rev-parse --absolute-git-dir>/zrush-session`. Per-worktree by
//! construction, so several editor windows can each hold their own session.
//! No global state, no secrets.

#![forbid(unsafe_code)]
// A test that cannot unwrap is a test that hides its own failure. The
// non-test build of this same crate still denies both.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod agent;
pub mod app;
pub mod config;
pub mod error;
pub mod git;
pub mod host;
pub mod model;
pub mod state;
