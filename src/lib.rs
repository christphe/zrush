//! zrush — pick a git worktree, or one of its Claude Code sessions, and open
//! it in your editor.
//!
//! The association worktree <-> session lives in the worktree's own gitdir,
//! at `<git rev-parse --absolute-git-dir>/zrush-session`. Per-worktree by
//! construction, so several editor windows can each hold their own session.
//! No global state, no secrets.

#![forbid(unsafe_code)]
// A test that cannot unwrap is a test that hides its own failure. The
// non-test build of this same crate still denies both.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod claude;
pub mod config;
pub mod error;
pub mod git;
pub mod model;
pub mod state;
