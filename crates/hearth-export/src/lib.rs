//! JSON contract: Hearth writes, sc-langpatch reads.
//!
//! Hearth is the producer; sc-langpatch is the consumer. This crate
//! contains only the schema (serde types) and the conventional
//! file-path constant. No logic, no I/O, **no heavy deps** — sc-langpatch
//! depends on data shape, not behaviour, and must not be forced to pull
//! sc-holotable (the two repos can track different sc-holotable versions).
//!
//! Blueprint identity crosses the boundary as the **CIG hex-guid string**
//! (`blueprint_record_guid`). Both apps already render guids to this exact
//! canonical form, so string-matching is stable across sc-holotable
//! versions — and the contract stays a pure serde crate.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// Conventional path under `%APPDATA%/` (Windows) where Hearth writes
/// the owned-blueprints JSON. sc-langpatch reads from the same path
/// by default; users can override in langpatch's config.
pub const EXPORT_RELATIVE_PATH: &str = "hearth/exports/owned-blueprints.json";

/// The owned-blueprints contract. Hearth rewrites it in full on every
/// ownership change; sc-langpatch reads it to render owned recipes
/// differently (grey / hide / annotate — the consumer decides).
///
/// `owned` is the set of `blueprint_record_guid`s (CIG hex strings) the
/// active scope has marked owned (single account + platform in v1). The
/// file is replaced atomically and never partially updated, so there's no
/// migration story — pre-release the shape can change in lockstep with
/// sc-langpatch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedBlueprints {
    /// `blueprint_record_guid`s the user owns, as CIG hex strings.
    pub owned: HashSet<String>,
}
