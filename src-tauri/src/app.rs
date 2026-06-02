//! Application core: the shared [`state::AppState`], on-disk [`paths`], the
//! [`ipc`] command wiring, cross-cutting [`events`] helpers, and the process
//! [`lifecycle`] (logging, warmup, `run`).
//!
//! `lib.rs` re-exports the handful of items the rest of the crate (and the
//! `export-bindings` binary) reaches for; everything else stays `pub(crate)`.

pub(crate) mod events;
pub(crate) mod ipc;
pub(crate) mod lifecycle;
pub(crate) mod paths;
pub(crate) mod state;
