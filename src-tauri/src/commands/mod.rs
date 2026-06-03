//! Tauri IPC command handlers — the adapter boundary between the WebView and
//! the domain / storage layers.
//!
//! Grouped by domain; each command is a thin wrapper that resolves scope / db
//! via [`crate::AppState`] and calls into `hearth-storage` / `hearth-core` /
//! [`crate::sc_loader`]. Command surfaces that are inseparable from heavier
//! domain logic live with that logic instead, and are wired into
//! [`crate::ipc_builder`] from their own modules (the setting commands in
//! [`crate::settings`], the history import in [`crate::sensors::import`], the
//! live sync in [`crate::live_sync`]).

pub(crate) mod accounts;
pub(crate) mod blueprints;
pub(crate) mod catalog;
pub(crate) mod missions;
