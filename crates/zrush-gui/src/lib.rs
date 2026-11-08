//! zrush in a window.
//!
//! Every decision lives in `zrush-core`; this crate holds one `Tab` per
//! repository — the core's service, loader and snapshot for it — and the
//! egui views that draw them.

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod app;
pub mod conversation;
pub mod dialog;
pub mod panes;
pub mod repos;
pub mod settings;
pub mod status;
pub mod tab;
pub mod tree_view;

#[cfg(test)]
pub mod testing;
