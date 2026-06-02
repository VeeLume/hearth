//! v1.5 — Game.log tailing and automatic SC-state sensing.
//!
//! Kept in its own module from Stage 0 so when the log format inevitably
//! churns with a new SC patch, the blast radius is local and replaceable.
//!
//! # Shape
//!
//! - [`parse`] — pure, per-line recognisers (the format-fragile core, fully
//!   unit-tested against real log samples).
//! - [`tailer`] — the I/O layer that reads a `Game.log` into
//!   [`SensedEvent`]s: whole-file [`summarize_session`] / [`scan_reader`] and
//!   the incremental [`GameLogTailer`].
//!
//! Wiring into `AppState` (resolve blueprint name → guid via the catalog,
//! pollution-guard the session against the active account + platform, then
//! mark owned) lives with the commands — this module only turns log bytes
//! into [`SensedEvent`]s.

pub mod parse;
pub mod tailer;

pub use parse::SensedEvent;
pub use tailer::{GameLogTailer, SessionSummary, scan_reader, summarize_session};

use std::path::{Path, PathBuf};

/// The conventional `Game.log` path inside an install's channel directory
/// (e.g. `…/StarCitizen/LIVE/Game.log`).
pub fn game_log_path(channel_dir: &Path) -> PathBuf {
    channel_dir.join("Game.log")
}

/// The directory of rotated session logs (`…/StarCitizen/LIVE/logbackups/`).
pub fn log_backups_dir(channel_dir: &Path) -> PathBuf {
    channel_dir.join("logbackups")
}
