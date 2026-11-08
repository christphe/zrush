//! zrush in a window.
//!
//! Every decision lives in `zrush-core`; this crate holds one `Tab` per
//! repository — the core's service, loader and snapshot for it — and the
//! egui views that draw them.

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod app;
pub mod repos;
pub mod tab;

#[cfg(test)]
pub mod testing;
