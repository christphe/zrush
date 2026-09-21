//! The terminal interface.
//!
//! Everything here draws, reads keys or spawns processes. The decisions
//! live in `zrush-core`, which this crate drives and which knows nothing
//! about ratatui, crossterm or clap.

#![forbid(unsafe_code)]
// A test that cannot unwrap is a test that hides its own failure. The
// non-test build of this same crate still denies both.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod host;
pub mod probe;

#[cfg(test)]
pub mod testing;
