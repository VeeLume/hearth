//! sqlx + SQLite repos for Hearth.
//!
//! Same crate, two consumers:
//! - Desktop (`hearth-app`): local cache + outgoing-write queue (`outbox`).
//! - Server (`hearth-server`, v2+): canonical state of record.
//!
//! Stage 1 fills in: migrations for `owned_blueprints`, `mission_completions`,
//! `wishlist_entries`, and a reserved-but-empty `outbox` table for v2 sync.
