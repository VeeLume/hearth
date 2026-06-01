//! JSON contract: Hearth writes, sc-langpatch reads.
//!
//! Hearth is the producer; sc-langpatch is the consumer. This crate
//! contains only the schema (serde types) and the conventional
//! file-path constant. No logic, no I/O — sc-langpatch depends on
//! data shape, not behaviour.
//!
//! Blueprint identity is sc-holotable's [`Guid`], which both apps already
//! depend on, so the consumer matches against the exact same key type
//! (no string round-tripping at the boundary). `Guid` serializes as its
//! hex string, so the on-disk JSON is a plain readable array.

use std::collections::HashSet;

use sc_holotable::asset::Guid;
use serde::{Deserialize, Serialize};

/// Conventional path under `%APPDATA%/` (Windows) where Hearth writes
/// the owned-blueprints JSON. sc-langpatch reads from the same path
/// by default; users can override in langpatch's config.
pub const EXPORT_RELATIVE_PATH: &str = "hearth/exports/owned-blueprints.json";

/// The owned-blueprints contract. Hearth rewrites it in full on every
/// ownership change; sc-langpatch reads it to render owned recipes
/// differently (grey / hide / annotate — the consumer decides).
///
/// `owned` is the set of `blueprint_record_guid`s the active scope has
/// marked owned (single account + platform in v1). The file is replaced
/// atomically and never partially updated, so there's no migration story —
/// pre-release the shape can change in lockstep with sc-langpatch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedBlueprints {
    /// `blueprint_record_guid`s the user owns.
    pub owned: HashSet<Guid>,
}
