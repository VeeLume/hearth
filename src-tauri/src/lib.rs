//! Tauri shell for Hearth — the desktop app crate.
//!
//! Thin by design: this file is module wiring plus the public re-exports the
//! `export-bindings` binary, `main.rs`, and the examples depend on.
//!
//! - [`app`] — the application core: shared state, on-disk paths, IPC wiring,
//!   cross-cutting event helpers, and the process lifecycle (`run`).
//! - [`commands`] — the Tauri IPC command handlers (the adapter boundary),
//!   grouped by domain.
//! - the remaining modules are the domains: SC reference data ([`sc_loader`]),
//!   Game.log sensing ([`sensors`] + [`sensing`] + [`import`]), RSI identity
//!   ([`identity`]), live sync ([`live_sync`]), preferences ([`settings`]),
//!   langpatch export ([`export`]), notifications ([`notify`]), and the shared
//!   error type ([`error`]).

mod app;

pub mod error;
pub mod export;
pub mod identity;
pub mod notify;
pub mod sc_loader;
pub mod sensors;

mod bp_resolve;
mod commands;
mod import;
mod live_sync;
mod sensing;
mod settings;

// Public API — consumed by the `export-bindings` binary, `main.rs`, and the
// examples (which also reach `sc_loader::{discover, build_data}` directly).
pub use app::ipc::{export_bindings, ipc_builder};
pub use app::lifecycle::run;

// Crate-internal essentials, re-exported at the root so the many
// `crate::AppState` / `crate::app_data_root` / `crate::emit_*` / `crate::plural`
// references across the domain modules resolve without each importing from
// `app::*`.
pub(crate) use app::events::{emit_ownership_changed, emit_scope_changed, plural, preview_names};
pub(crate) use app::paths::app_data_root;
pub(crate) use app::state::AppState;
