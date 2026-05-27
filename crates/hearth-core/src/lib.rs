//! Hearth domain types and logic. Pure — no transport, no storage, no I/O.
//!
//! Types here are linked by both desktop (`hearth-app`) and server
//! (`hearth-server`, v2+) so domain stays single-source-of-truth.

pub mod sc_data;
pub mod types;

pub use types::{BpView, MissionCompletion, OwnedBlueprint, RecordId, WishlistEntry};
