//! Domain types for Hearth.
//!
//! - `Account` is the RSI-account scope key for every personal-state row.
//!   Hearth-local UUIDv7 PK; the `handle` is the user-facing display;
//!   `citizen_record` + `enlisted` are immutable anchors scraped from
//!   `/citizens/<handle>` once the user signs in (left `None` for the
//!   no-sign-in default path).
//! - `OwnedBlueprint`, `WishlistEntry` are the persisted personal-state
//!   records. Scoped by `(account_id, platform)`. (Mission completion is
//!   *derived* from BP ownership, not stored.)
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
/// what `sc_discovery::Installation::platform_id` produces.
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

    /// Parse from the storage form ("prod" / "ptu") or from sc-discovery's
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
    /// `Guid` from sc-extract, rendered as the hex string sc-holotable
    /// uses externally. String here so the type stays serde/specta-friendly
    /// without pulling sc-extract into the IPC layer.
    pub blueprint_guid: String,
    pub platform: Platform,
    /// FK to `accounts.id`.
    pub account_id: RecordId,
    pub owned_at: DateTime<Utc>,
}

/// What a wishlist entry expresses. A blueprint↔item is 1:1, but "I want
/// this" is ambiguous between wanting the recipe and wanting a crafted
/// copy — two independent goals a single BP can carry at once.
///
/// Storage form is the lowercase string (`'recipe'` / `'item'`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum WishIntent {
    /// Want the blueprint/recipe itself — the capability to craft it
    /// (collector goal, or a prerequisite). Only meaningful while the BP
    /// is *unowned* (owning it means you already have the recipe).
    /// Fulfilled by mission rewards.
    Recipe,
    /// Want a crafted copy of the item in hand. Meaningful regardless of
    /// ownership (own the BP → craft it; don't → learn it first).
    /// Fulfilled by crafting (self, v1.5) or community (v2).
    Item,
}

impl WishIntent {
    /// Storage representation — also the serde rename.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recipe => "recipe",
            Self::Item => "item",
        }
    }

    /// Parse from the storage form. `Option` (not `FromStr`) so an unknown
    /// value is a soft failure at the row-mapping boundary.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "recipe" => Some(Self::Recipe),
            "item" => Some(Self::Item),
            _ => None,
        }
    }
}

/// A blueprint↔item the user wants, carrying a single [`WishIntent`].
/// Unique by `(blueprint_guid, intent, platform, account_id)` — the two
/// intents coexist as separate rows for one BP.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct WishlistEntry {
    pub id: RecordId,
    pub blueprint_guid: String,
    pub intent: WishIntent,
    pub platform: Platform,
    /// FK to `accounts.id`.
    pub account_id: RecordId,
    pub added_at: DateTime<Utc>,
}

/// Lean view of a mission/contract for the Missions UI.
///
/// Built from `sc_missions::Mission` (+ its `BlueprintPools`) in the loader.
/// Focused on the reward axes hearth surfaces — blueprints first, plus the
/// common reward kinds (aUEC, scrip, reputation, item unlocks). Heavy mission
/// detail (encounters, prerequisites, localities, factions-by-name) is
/// intentionally omitted; reputation carries the raw faction GUID for now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct MissionView {
    /// Contract GUID, hex-string form. Stable id for UI keys.
    pub mission_id: String,
    /// Resolved title; `None` → the UI falls back to `debug_name`.
    pub title: Option<String>,
    /// Internal contract debug name — fallback label + DCB cross-ref.
    pub debug_name: String,
    /// Resolved description. May contain `~mission(...)` runtime-substitution
    /// markers the engine fills at spawn time.
    pub description: Option<String>,
    /// `availability.once_only` — non-repeatable.
    pub once_only: bool,
    pub shareable: bool,
    pub illegal: bool,
    /// Post-completion personal cooldown in seconds, if any.
    pub cooldown_seconds: Option<f32>,
    /// Fixed aUEC payout, when the contract pays a fixed amount.
    pub uec_fixed: Option<i32>,
    /// True when the aUEC payout is engine-computed at runtime (amount unknown).
    pub uec_calculated: bool,
    pub scrip: Vec<ScripRewardView>,
    pub reputation: Vec<RepRewardView>,
    pub item_rewards: Vec<ItemRewardView>,
    /// Blueprint-pool rewards — each a weighted pool the contract draws from.
    pub blueprint_rewards: Vec<BpPoolReward>,
}

/// A typed-currency (scrip) reward on a mission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ScripRewardView {
    /// Resolved currency display name; `None` if it didn't resolve.
    pub name: Option<String>,
    pub amount: i32,
}

/// A reputation reward on a mission. Faction is a raw GUID for now (name
/// resolution is a follow-up); `amount` is `None` for engine-calculated rep.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RepRewardView {
    pub faction_guid: Option<String>,
    pub amount: Option<i32>,
}

/// A non-currency item reward (ship unlock, collector item, …).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ItemRewardView {
    pub entity_guid: String,
    /// Resolved item display name; `None` if it didn't resolve.
    pub name: Option<String>,
    pub amount: i32,
}

/// One blueprint-pool reward on a mission: a weighted set the contract draws
/// from, with the chance the draw happens at all.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct BpPoolReward {
    /// `BlueprintPoolRecord` name (prefix stripped). Empty if unnamed.
    pub pool_name: String,
    /// 0.0–1.0 chance the blueprint draw happens.
    pub chance: f32,
    /// Weighted entries in the pool, descending weight.
    pub blueprints: Vec<BpRewardEntry>,
}

/// One weighted blueprint inside a [`BpPoolReward`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct BpRewardEntry {
    /// `blueprint_record_guid` — matches `BpView.blueprint_record_guid` and
    /// the wishlist/ownership key, so the UI can cross-reference.
    pub blueprint_record_guid: String,
    /// Resolved crafted-item display name; `None` if it didn't resolve.
    pub name: Option<String>,
    /// Relative pick-weight within the pool (higher = more likely).
    pub weight: f32,
}

/// Lean view of a craftable blueprint for the catalog UI.
///
/// Constructed by `sc_data::bp_view`. All GUIDs are rendered as their
/// hex-string form for the IPC boundary — the Svelte side never sees a
/// raw `Guid`.
///
/// The catalog source is the full `sc_crafting::Blueprints` index, not
/// the mission-reward pool registry — every craftable blueprint
/// appears, including default-unlocked ones (P4-AR, basic dismantle,
/// etc) that are in no pool. Mission-pool data (pool_guid, pool_name,
/// drop weight) is a *mission-reward* mechanic and lives in the future
/// Missions view, not here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct BpView {
    pub blueprint_record_guid: String,
    pub crafted_entity_guid: Option<String>,
    /// Resolved display name. `None` when LocaleMap doesn't resolve;
    /// UI should fall back to the GUID.
    pub display_name: Option<String>,
    /// Raw CIG item-type classification of the crafted entity
    /// (`AttachDef.Type`), e.g. `"Char_Armor_Helmet"`, `"WeaponPersonal"`.
    /// `None` when the crafted entity didn't resolve or carried no type.
    /// The UI maps these raw strings into friendly catalog categories.
    pub item_type: Option<String>,
    /// Raw `AttachDef.SubType` (e.g. `"Rifle"`). `None` on most items.
    /// Available for finer sub-grouping; not all items set it.
    pub item_sub_type: Option<String>,
    /// sc-crafting category record name with the
    /// `"BlueprintCategoryRecord."` prefix stripped — e.g. `"FPSArmours"`,
    /// `"FPSWeapons"`, `"VehicleWeaponsS3"`, `"Medical"`. `None` when
    /// the blueprint has no category reference. CIG-authored, more
    /// accurate than the AttachDef-based taxonomy for "what kind of
    /// craftable is this" — the UI uses it as the primary grouping
    /// axis with item_type as the secondary axis (slot for armor,
    /// size class for FPS weapons).
    pub category_raw: Option<String>,
    /// Stable identity key used by the UI to bundle skin / paint /
    /// special-edition variants of the same base item under a single
    /// collapsible row. Comes from
    /// [`sc_holotable::items::ItemCatalog::model_id_of`] — a
    /// display-name-derived model key
    /// (`"{category}:{design}:{item_type}:{item_sub_type}"`, e.g.
    /// `"armor:geist armor:Char_Armor_Helmet:UNDEFINED"`) or a
    /// `"solo:<guid>:..."` for gear with no other signal. Falls back to
    /// the raw guid for non-gear entities (handled in the loader).
    pub family_id: Option<String>,
    /// Display name of the model's **base** item — the canonical
    /// unstyled variant identified by [`sc_holotable::items::Model::base`].
    /// Shared across every BpView in a model; used by the catalog UI
    /// as the bundle row header so the row reads the base item's name
    /// even when only variants are blueprinted (the base item itself
    /// might have no recipe). `None` when the BP has no crafted
    /// entity, or when the base entity has no resolvable display name
    /// — the UI then falls back to the shortest blueprinted name in
    /// the bundle.
    pub family_base_name: Option<String>,
    /// Crafting recipe — ingredients + craft time. `None` when the
    /// blueprint has no recipe (rare; happens when the cost tree is a
    /// dormant variant the live data doesn't populate) or when the
    /// crafted item has no recoverable ingredient list.
    pub recipe: Option<Recipe>,
}

/// Flat projection of a `sc_crafting::Blueprint.tiers[0].recipe` shaped
/// for the catalog UI. Today's SC 4.8 data is uniformly
/// `Select { N, [Select { 1, [Resource] }] }`, which we flatten to a
/// straight `Vec<Ingredient>`. The polymorphic cost tree stays in
/// sc-crafting as forward-compat for when CIG ships item costs /
/// optional costs / dormant variants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct Recipe {
    /// Total craft time normalized to seconds (from
    /// `TimeValue_Partitioned.{days,hours,minutes,seconds}`). `None`
    /// when the blueprint has no time component.
    pub craft_time_seconds: Option<f32>,
    /// Each resource the recipe needs. Order matches the DCB's
    /// declared cost order.
    pub ingredients: Vec<Ingredient>,
}

/// One resource ingredient in a [`Recipe`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct Ingredient {
    /// `ResourceType` GUID, hex-string form.
    pub resource_guid: String,
    /// Resolved resource name (e.g. `"Aluminum"`). `None` when the
    /// resource's `name_key` doesn't resolve in the locale map (rare
    /// — SC 4.8 resolves 205 / 206).
    pub resource_name: Option<String>,
    /// Quantity normalized to SCU (Standard Cargo Units). `None` when
    /// the cost's `CargoQuantity` is a polymorphic-fallback variant
    /// the generator doesn't recognise. Typical recipe ingredients are
    /// well under 1 SCU each (e.g. P4-AR: Aluminum 0.04, Iron 0.02).
    pub quantity_scu: Option<f32>,
    /// Minimum required quality tier (`0` if no lower bound). Today
    /// always 0 in SC 4.8.
    pub min_quality: i32,
}
