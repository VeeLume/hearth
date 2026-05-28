//! Domain types for Hearth.
//!
//! - `Account` is the RSI-account scope key for every personal-state row.
//!   Hearth-local UUIDv7 PK; the `handle` is the user-facing display;
//!   `citizen_record` + `enlisted` are immutable anchors scraped from
//!   `/citizens/<handle>` once the user signs in (left `None` for the
//!   no-sign-in default path).
//! - `OwnedBlueprint`, `MissionCompletion`, `WishlistEntry` are the
//!   persisted personal-state records. Scoped by `(account_id, platform)`.
//! - `BpView` is the lean shape sent over the Tauri IPC boundary for the
//!   blueprint catalog UI.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

/// Which SC services environment a piece of personal state belongs to.
///
/// Matches CIG's launcher-store `platform_id` value verbatim: Live and
/// Hotfix run on `prod` (the persistent universe; long-lived state).
/// PTU, EPTU, and Tech Preview run on `ptu` test shards that wipe
/// regularly. Strict separation keeps test-shard progress from
/// polluting PU state.
///
/// Storage form is the lowercase string (`'prod'` / `'ptu'`) — same as
/// what `sc_installs::Installation::platform_id` produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    /// Production / persistent universe (Live + Hotfix).
    Prod,
    /// Test shards (PTU + EPTU + Tech Preview). Wipes frequently.
    Ptu,
}

impl Platform {
    /// Storage representation — also the serde rename.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Prod => "prod",
            Self::Ptu => "ptu",
        }
    }

    /// Parse from the storage form ("prod" / "ptu") or from sc-installs'
    /// `Installation::platform_id` value (same shape). Inherent rather
    /// than `FromStr` because we want `Option` (unknown variants are
    /// soft failures), not `Result`.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "prod" => Some(Self::Prod),
            "ptu" => Some(Self::Ptu),
            _ => None,
        }
    }
}

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

/// An RSI account this desktop has seen. One row per distinct account
/// (multi-account on the same desktop = multiple rows). Personal-state
/// tables FK back to this via `account_id`.
///
/// `handle` is the current display name; `citizen_record` + `enlisted`
/// are the truly-immutable public anchors scraped from
/// `/citizens/<handle>` once the user explicitly verifies. They stay
/// `None` for the no-sign-in default flow where Hearth just trusts the
/// launcher store's nickname.
///
/// `account_hint` (optional) stores the launcher's `heapAccountId` /
/// in-log `accountId` value. Use ONLY as a cross-reference key when
/// parsing Game.log — never as the storage primary key. CIG has been
/// observed to rotate this value during backend migrations (Oct→Dec
/// 2024), so it's not a stable identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Account {
    pub id: RecordId,
    /// Current RSI handle, e.g. `"VeeLume"`. Mutable — user can rename
    /// via a paid handle change. Don't use as a join key.
    pub handle: String,
    /// UEE citizen record number from the public profile (`#1196670`).
    /// Immutable, assigned at account creation. `None` until the user
    /// triggers a profile-verify scrape.
    pub citizen_record: Option<i64>,
    /// Account creation date from the public profile (`"2016-01-31"`).
    /// Immutable. `None` until verified.
    pub enlisted: Option<String>,
    /// When the profile was last successfully scraped + verified.
    /// `None` if never verified. Drives the "re-verify if stale" decay.
    pub last_verified: Option<DateTime<Utc>>,
    /// Launcher / in-log numeric account id (CIG's `heapAccountId`).
    /// Optional cross-reference for log parsing. Never used as a key.
    pub account_hint: Option<i64>,
    pub created_at: DateTime<Utc>,
}

/// A blueprint the user has acquired in-game. Unique by
/// `(blueprint_guid, platform, account_id)` — the same BP can be
/// independently "owned" on prod and on a test shard, and across
/// multiple RSI accounts on the same desktop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OwnedBlueprint {
    pub id: RecordId,
    /// `Guid` from sc-extract, rendered as the hex string `sc-contracts`
    /// uses externally. String here so the type stays serde/specta-friendly
    /// without pulling sc-extract into the IPC layer.
    pub blueprint_guid: String,
    pub platform: Platform,
    /// FK to `accounts.id`.
    pub account_id: RecordId,
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
    pub platform: Platform,
    /// FK to `accounts.id`.
    pub account_id: RecordId,
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
    pub platform: Platform,
    /// FK to `accounts.id`.
    pub account_id: RecordId,
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
    /// Resolved display name. `None` when LocaleMap doesn't resolve;
    /// UI should fall back to the GUID.
    pub display_name: Option<String>,
    pub weight: f32,
}
