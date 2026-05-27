//! Domain types for Hearth.
//!
//! - `OwnedBlueprint`, `MissionCompletion`, `WishlistEntry` are the
//!   persisted personal-state records. UUIDv7 IDs (sortable, no central
//!   authority, work offline — see vault `#IDs` section).
//! - `BpView` is the lean shape sent over the Tauri IPC boundary for the
//!   blueprint catalog UI. Constructed by `sc_data` from a sc-contracts
//!   `BlueprintItem` plus its containing pool.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

/// Stable record-level identifier. UUIDv7 so it's time-sortable and
/// generated client-side without a central authority. Wraps `Uuid` rather
/// than exposing it directly so the storage layer can swap to e.g. ULID
/// later without touching call sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct RecordId(pub Uuid);

impl RecordId {
    pub fn new_v7() -> Self {
        Self(Uuid::now_v7())
    }
}

impl std::fmt::Display for RecordId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A blueprint the user has acquired in-game. Unique by `blueprint_guid`
/// (you can't own the same BP twice).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OwnedBlueprint {
    pub id: RecordId,
    /// `Guid` from sc-extract, rendered as the hex string `sc-contracts`
    /// uses externally. String here so the type stays serde/specta-friendly
    /// without pulling sc-extract into the IPC layer.
    pub blueprint_guid: String,
    pub owned_at: DateTime<Utc>,
}

/// A completed mission with optionally-collected non-repeatable rewards.
/// Many missions are repeatable; this tracks "I've done this mission and
/// claimed/missed these specific BP rewards." A separate child table in
/// storage (`mission_rewards_collected`) holds which rewards were grabbed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct MissionCompletion {
    pub id: RecordId,
    pub mission_id: String,
    pub completed_at: DateTime<Utc>,
    /// `blueprint_guid`s of non-repeatable BP rewards the user collected
    /// when they did this mission.
    pub rewards_collected: Vec<String>,
}

/// A blueprint the user wants. Surfaces in lists / craft suggestions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct WishlistEntry {
    pub id: RecordId,
    pub blueprint_guid: String,
    pub added_at: DateTime<Utc>,
}

/// Lean view of a sc-contracts `BlueprintItem` for the catalog UI.
///
/// Constructed by `sc_data::bp_view`. All GUIDs are rendered as their
/// hex-string form for the IPC boundary — the Svelte side never sees a
/// raw `Guid`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct BpView {
    pub pool_guid: String,
    pub pool_name: String,
    pub blueprint_record_guid: String,
    pub crafted_entity_guid: Option<String>,
    /// Resolved display name. `None` until Stage 2 wires LocaleMap-driven
    /// name resolution; UI should fall back to the GUID for now.
    pub display_name: Option<String>,
    pub weight: f32,
}
