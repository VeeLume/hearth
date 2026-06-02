//! IPC-friendly error type for Hearth Tauri commands.
//!
//! `thiserror` for the source-of-truth definition; `specta::Type` so the
//! Svelte side gets a typed union; `Serialize` so Tauri can return it.

use serde::Serialize;
use specta::Type;
use thiserror::Error;

#[derive(Debug, Error, Serialize, Type)]
#[serde(tag = "kind", content = "message")]
pub enum AppError {
    #[error("storage error: {0}")]
    Storage(String),
    #[error("no SC install detected: {0}")]
    NoInstall(String),
    #[error("identity scrape failed: {0}")]
    Identity(String),
    #[error("live sync failed: {0}")]
    LiveSync(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        Self::Internal(format!("{err:#}"))
    }
}
