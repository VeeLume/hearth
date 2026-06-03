//! v1.5 — Game.log tailing and automatic SC-state sensing.
//!
//! Kept in its own module from Stage 0 so when the log format inevitably
//! churns with a new SC patch, the blast radius is local and replaceable.
//!
//! # Shape
//!
//! The format-fragile mechanism (kept small so a log-format break is a local
//! fix):
//! - [`parse`] — pure, per-line recognisers (fully unit-tested against real
//!   log samples).
//! - [`tailer`] — the I/O layer that reads a `Game.log` into [`SensedEvent`]s:
//!   whole-file [`summarize_session`] / [`scan_reader`] and the incremental
//!   [`GameLogTailer`].
//!
//! The app-side consumers that wire the mechanism to `AppState`, the DB, the
//! catalog name index, and the notification funnel:
//! - [`live`] — the live polling task that auto-marks blueprints owned during
//!   play (pollution-guarded against the active account + platform).
//! - [`import`] — the log-history import (scan backups → group identities →
//!   mark owned), plus its Tauri commands.
//! - [`resolve`] — resolve a received-blueprint display name to its catalog
//!   `blueprint_record_guid`s (shared by `live` and `import`).

pub mod parse;
pub mod tailer;

pub(crate) mod import;
pub(crate) mod live;
pub(crate) mod resolve;

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
