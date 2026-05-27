//! JSON contract: Hearth writes, sc-langpatch reads.
//!
//! Hearth is the producer; sc-langpatch is the consumer. This crate
//! contains only the schema (serde types) and the conventional
//! file-path constant. No logic, no I/O — sc-langpatch depends on
//! data shape, not behaviour.
//!
//! `OwnedBlueprints` type is filled in at Stage 4.

/// Conventional path under `%APPDATA%/` (Windows) where Hearth writes
/// the owned-blueprints JSON. sc-langpatch reads from the same path
/// by default; users can override in langpatch's config.
pub const EXPORT_RELATIVE_PATH: &str = "hearth/exports/owned-blueprints.json";
