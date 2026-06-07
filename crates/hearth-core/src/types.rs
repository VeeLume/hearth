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

/// Lean view of a mission **template** for the Missions UI.
///
/// Built from `sc_missions::Mission` (+ its `BlueprintPools` / `Localities`)
/// in the loader. CIG spawns one contract per offered locality, so the raw
/// list has thousands of near-duplicates; the loader **pools** contracts that
/// share a `(title_key, description_key)` into one template (the mission a
/// player perceives), aggregating the localities into [`Self::regions`].
/// Reward axes are surfaced for the blueprint focus (blueprints first), plus
/// aUEC / scrip / reputation / item unlocks. Reputation carries the raw
/// faction GUID for now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct MissionView {
    /// Representative contract GUID, hex-string form. Stable id for UI keys.
    pub mission_id: String,
    /// Resolved title; `None` → the UI falls back to `debug_name`.
    /// `~mission(Var)` runtime markers are rendered as readable `[Var]`.
    pub title: Option<String>,
    /// Internal contract debug name — fallback label + DCB cross-ref.
    pub debug_name: String,
    /// Resolved description, with `~mission(Var)` → `[Var]`. Real line breaks
    /// (the locale stores them as the literal two-char `\n`).
    pub description: Option<String>,
    /// Mission category (SCMDB's "Mission Type" — Bounty Hunter / Hauling /
    /// Mercenary / Salvage / …), resolved from the template's display info.
    /// `None` when the mission has no category.
    pub category: Option<MissionCategoryView>,
    /// Reputation faction (SCMDB's "Faction") — the giving / gaining faction
    /// the UI groups + filters by. `None` when the mission touches no faction.
    pub faction: Option<FactionView>,
    /// Difficulty profile (the four 1–8 axes). Players don't see this in-game;
    /// it drives the computed payout. `None` when no difficulty is authored.
    pub difficulty: Option<DifficultyView>,
    /// aUEC payout — fixed amount, evergr3n-estimate, or engine-calculated,
    /// plus the buy-in and time budget. See [`PayoutView`].
    pub payout: PayoutView,
    /// `availability.once_only` — non-repeatable.
    pub once_only: bool,
    pub shareable: bool,
    pub illegal: bool,
    /// Post-completion personal cooldown in seconds, if any.
    pub cooldown_seconds: Option<f32>,
    pub scrip: Vec<ScripRewardView>,
    pub reputation: Vec<RepRewardView>,
    pub item_rewards: Vec<ItemRewardView>,
    /// Blueprint-pool rewards — each a weighted pool the contract draws from
    /// (union across the pooled contracts, deduped by pool).
    pub blueprint_rewards: Vec<BpPoolReward>,
    /// Reputation a player must hold to accept (faction + standing-tier range).
    /// Empty when ungated.
    pub rep_required: Vec<RepRequirementView>,
    /// Missions that must be completed first to unlock this one (the chain
    /// gate), resolved from completion-tag prerequisites. Empty when ungated.
    pub chain_required: Vec<MissionRef>,
    /// Where the mission is offered — grouped by star system, each place
    /// carrying its typed kind (station / planet / outpost / …). The UI's
    /// system sub-split + position-type axis come from this.
    pub locations: Vec<RegionView>,
    /// Structured encounters — ships (with counts + resolved cargo) and the
    /// difficulty class. Empty when the mission has no ship/entity encounter.
    pub encounters: Vec<EncounterView>,
    /// `~mission(Var)` runtime-substitution variable names present in the
    /// title / description (e.g. `["Location", "CargoGradeToken"]`). The UI
    /// can cross-reference them against `locations` / `encounters`.
    pub placeholders: Vec<String>,
    /// How many raw contract expansions this entry collapses (offered-at-N).
    pub instance_count: u32,
}

/// Resolved mission category — name + optional icon hints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct MissionCategoryView {
    /// Display name (`"Hauling"`). `None` if the locale didn't resolve.
    pub name: Option<String>,
    /// `MissionType.IconName` — UI icon id, empty string when none.
    pub icon: String,
}

/// Resolved reputation faction — stable GUID key + display name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct FactionView {
    /// `FactionReputation` GUID hex — stable grouping/filter key.
    pub guid: String,
    /// Display name (`"Ling Family Hauling"`). `None` if it didn't resolve.
    pub name: Option<String>,
}

/// The four authored difficulty axes, each a `1..=8` level (`0` = unparsed).
/// Hidden from players; surfaced for tooltip / sort, and the payout's driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DifficultyView {
    pub mechanical_skill: u8,
    pub mental_load: u8,
    pub risk_of_loss: u8,
    pub game_knowledge: u8,
}

/// aUEC payout, the visible reward axis the UI groups/sorts by.
///
/// The static DCB rarely stores a number for generated contracts — the engine
/// computes it from the difficulty profile (the community "evergr3n" formula
/// SCMDB uses). Until that estimator lands, `estimate` is `None` and the UI
/// shows "calculated". A `fixed` amount wins when the contract hardcodes one.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Type)]
pub struct PayoutView {
    /// True when the reward is engine-calculated (`ContractResult_CalculatedReward`).
    pub calculated: bool,
    /// A hardcoded fixed aUEC amount, when the contract carries one.
    pub fixed: Option<i32>,
    /// Estimated aUEC: `round₂₅₀(1232 · 1.354^weighted_difficulty · minutes)`,
    /// the exponential-in-difficulty payout curve. `None` for fixed/absent
    /// payouts or when difficulty inputs are missing.
    pub estimate: Option<i32>,
    /// Upfront cost to accept (`contractBuyInAmount`), 0 when free.
    pub buy_in: i32,
    /// Time budget in minutes (`timeToComplete`), 0 when none.
    pub time_to_complete: f32,
}

/// One reputation-acceptance requirement — a faction and the standing-tier
/// window the player must sit in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct RepRequirementView {
    /// Faction display name. `None` if it didn't resolve.
    pub faction: Option<String>,
    /// Lower standing-tier bound (`"Neutral"`), resolved name. `None` if unbounded.
    pub min_rank: Option<String>,
    /// Upper standing-tier bound. `None` if unbounded.
    pub max_rank: Option<String>,
    /// Numeric tier index of [`Self::min_rank`] (`0`–`6` on the generic
    /// `FactionRep` scale), for ordering / range filters. `None` if unbounded
    /// or unparsed. (Groundwork for the sc-dossier "missions I can accept"
    /// filter, which will compare against the player's live standing.)
    pub min_rank_index: Option<i32>,
    /// Numeric tier index of [`Self::max_rank`]. `None` if unbounded.
    pub max_rank_index: Option<i32>,
    /// True for an *exclusion* requirement (must NOT be in this range).
    pub exclude: bool,
}

/// One locality's worth of accept-locations — the parent "available in" card
/// (e.g. *Stanton — Hurston*, *Pyro — Region A*), expandable to its places.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct RegionView {
    /// System display name (`"Stanton"`, `"Pyro"`, `"Nyx"`).
    pub system: String,
    /// Locality name (`"Hurston"`, `"Region A"`) — the parent grouping a
    /// mission is offered in. Empty when the locality has no name.
    pub name: String,
    /// The places within this locality the mission is offered at.
    pub places: Vec<PlaceView>,
}

/// One accept-location — a resolved place + its typed kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct PlaceView {
    /// Display name (`"Bloom"`, `"microTech"`). `None` if it didn't resolve;
    /// the UI falls back to [`Self::record_name`].
    pub name: Option<String>,
    /// Stable record-name stem (`"Pyro3"`), always present.
    pub record_name: String,
    /// Typed `LocationKind` as a string (`"Planet"`, `"Station"`, `"Outpost"`,
    /// …) — the in-game position type. `None` when unresolved.
    pub kind: Option<String>,
}

/// One encounter the mission spawns — its difficulty class and waves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct EncounterView {
    /// The encounter's mission-variable name (`"AmbushTarget"` / `"Wave"` / …).
    pub label: String,
    /// Combat-class difficulty tag (`"VeryEasy"` … `"Hard"`), when uniform
    /// across the encounter. `None` otherwise.
    pub difficulty: Option<String>,
    /// Ordered waves/phases.
    pub waves: Vec<WaveView>,
}

/// One wave/phase of an encounter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct WaveView {
    /// Wave name (`"Wave1"` / `"SalvageableShip"` / empty).
    pub name: String,
    /// Ship slots in this wave.
    pub ships: Vec<ShipSlotView>,
    /// Resolved cargo descriptors across the wave's slots (deduped).
    pub cargo: Vec<String>,
}

/// One ship slot — how many of which candidate ships, and their factions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct ShipSlotView {
    /// Concurrent-spawn count range across the slot's alternatives.
    pub count_min: i32,
    pub count_max: i32,
    /// Candidate ship display names (the engine picks one per spawn).
    pub ships: Vec<String>,
    /// Faction descriptors on the slot (deduped).
    pub factions: Vec<String>,
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

/// A reference to a mission that grants a blueprint — the lean shape the
/// wishlist's ⚐ fulfilment slot needs to answer "which missions grant this
/// BP?". Derived (not stored) by inverting the cooked [`MissionView`] list;
/// see [`crate::missions::missions_by_blueprint`]. Inverting the *pooled*
/// `MissionView`s (rather than sc-missions' raw `missions_for_item`) keeps
/// these refs consistent with the templates the Missions view renders —
/// same `mission_id`, same title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct MissionRef {
    /// Matches [`MissionView::mission_id`] so the UI can cross-reference the
    /// Missions view.
    pub mission_id: String,
    /// Resolved mission title; `None` → UI falls back to the id / debug name.
    pub title: Option<String>,
    /// Mirrors [`MissionView::once_only`]. A non-repeatable source is worth
    /// flagging — the BP can only be earned from it once.
    pub once_only: bool,
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

/// Whether a recipe ingredient is a bulk resource or a discrete item.
///
/// SC recipes mix two cost kinds: `Resource` ingredients are
/// ship-mined / refined materials measured in SCU cargo
/// (`CraftingCost_Resource` → `ResourceType`); `Item` ingredients are
/// discrete carried entities measured as a unit count
/// (`CraftingCost_Item` → `EntityClassDefinition`) — the hand-mined
/// gems (Hadanite, …) live here. SC 4.8 has both in real recipes, so
/// the catalog must surface both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum IngredientKind {
    /// Bulk resource — quantity in SCU (see [`Ingredient::quantity_scu`]).
    Resource,
    /// Discrete item — quantity is a unit count (see [`Ingredient::count`]).
    Item,
}

impl IngredientKind {
    /// Storage representation — also the serde rename. Shared by recipe
    /// ingredients and inventory stacks (both split on the same axis).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Resource => "resource",
            Self::Item => "item",
        }
    }

    /// Parse from the storage form. `Option` (not `FromStr`) so an unknown
    /// value is a soft failure at the row-mapping boundary.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "resource" => Some(Self::Resource),
            "item" => Some(Self::Item),
            _ => None,
        }
    }
}

/// One ingredient in a [`Recipe`] — either a bulk resource or a discrete
/// item, discriminated by [`Ingredient::kind`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct Ingredient {
    /// Which cost kind this ingredient is. Determines whether quantity
    /// is read from `quantity_scu` (`Resource`) or `count` (`Item`), and
    /// what `guid` points at.
    pub kind: IngredientKind,
    /// Source GUID, hex-string form. A `ResourceType` GUID when `kind`
    /// is [`IngredientKind::Resource`], an `EntityClassDefinition` GUID
    /// when it's [`IngredientKind::Item`].
    pub guid: String,
    /// `class_crc` of [`Self::guid`] — the CRC32C the EntityGraph backend
    /// (and sc-dossier) identifies this resource/item by on the wire. Lets
    /// the frontend match a recipe ingredient against the player's live
    /// inventory ([`InventoryStack::crc`]) without re-hashing GUIDs. `None`
    /// when the GUID doesn't parse (the name/coverage UI then falls back to
    /// "untracked").
    pub crc: Option<u32>,
    /// Resolved display name (e.g. `"Aluminum"`, `"Hadanite"`). `None`
    /// when the source's `name_key` doesn't resolve in the locale map.
    pub name: Option<String>,
    /// Quantity normalized to SCU (Standard Cargo Units), for `Resource`
    /// ingredients. `None` for `Item` ingredients (use `count`), or when
    /// the cost's `CargoQuantity` is a polymorphic-fallback variant the
    /// generator doesn't recognise. Typical resource ingredients are
    /// well under 1 SCU each (e.g. P4-AR: Aluminum 0.04, Iron 0.02).
    pub quantity_scu: Option<f32>,
    /// Quantity as a discrete unit count, for `Item` ingredients (e.g.
    /// Hadanite ×13). `None` for `Resource` ingredients (use
    /// `quantity_scu`).
    pub count: Option<i32>,
    /// Minimum required quality tier (`0` if no lower bound).
    pub min_quality: i32,
}

/// The rich, per-slot crafting view for one blueprint — the data behind the
/// `/crafting` recipe calculator. Built from the same
/// `sc_crafting::Blueprint.tiers[0].recipe` as [`Recipe`], but instead of
/// flattening the cost tree it preserves the **named material slots**
/// (`CraftingCost_Select.name_info` → "Frame" / "Cabling" / …) and each
/// slot's **gameplay-property modifier curves** (how the chosen material's
/// quality reshapes the crafted item's stats). Fetched on demand per
/// blueprint (`get_craft_detail`), not embedded in the catalog's [`BpView`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct CraftDetail {
    /// The blueprint this detail is for (hex GUID — matches
    /// [`BpView::blueprint_record_guid`]).
    pub blueprint_record_guid: String,
    /// Total craft time normalized to seconds. Same value as
    /// [`Recipe::craft_time_seconds`].
    pub craft_time_seconds: Option<f32>,
    /// `CraftingGlobalParams.default_composition_quality` (500 in SC 4.8) —
    /// the "Base" reference quality the UI's presets and modifier curves
    /// anchor on. Global, repeated here so a single detail fetch is
    /// self-contained.
    pub default_quality: i32,
    /// One entry per named material slot, in the recipe's declared order.
    pub slots: Vec<RecipeSlot>,
}

/// One named material slot in a [`CraftDetail`] — a material plus the
/// gameplay-property effects its quality drives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct RecipeSlot {
    /// Resolved slot label ("Frame", "Cabling", "Power Regulator"). `None`
    /// for unnamed / placeholder slots (`<= PLACEHOLDER =>` in the DCB).
    pub slot_name: Option<String>,
    /// The material that fills this slot (resource or item) — same shape the
    /// catalog uses, with `min_quality` and resolved `name`.
    pub ingredient: Ingredient,
    /// Gameplay-property modifier curves attached to this slot. Empty when
    /// the slot drives no stats. Evaluated client-side against the slider
    /// quality (see `src/lib/domain/crafting.ts`).
    pub modifiers: Vec<CraftModifier>,
}

/// One gameplay property a slot's material quality reshapes, with the curve
/// to evaluate and the transform to display it. Projection of
/// `sc_crafting::GameplayPropertyModifier` joined to its
/// `CraftingGameplayPropertyDef` (display metadata).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct CraftModifier {
    /// Resolved property display name ("Recoil Smoothness", "Impact Force").
    /// `None` when the property GUID doesn't resolve. Note CIG's display name
    /// can differ from the record key.
    pub property_name: Option<String>,
    /// Resolved unit-format string (a printf template like `"%.2f RPM"`).
    /// `None` when empty / `@LOC_EMPTY`.
    pub unit_format: Option<String>,
    /// How to present the evaluated factor (percent change / scale / raw).
    pub transform: ModifierTransform,
    /// Quality→value bands. Usually one; evaluated by picking the band that
    /// contains the quality (else the nearest), then linearly interpolating.
    pub ranges: Vec<ModifierRange>,
}

/// Flattened `sc_crafting::DisplayTransformation` — how a modifier's raw
/// factor is shown. Kept as a tagged struct (not a data enum) for clean TS
/// bindings. `kind` ∈ `scale` | `factor_to_percent` |
/// `factor_to_negated_percent` | `value_to_factor` | `raw`. `raw` covers
/// the rare `Sequence` / dormant variants (show the ×factor, no percent).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct ModifierTransform {
    pub kind: String,
    /// Scalar for `kind == "scale"`; `None` otherwise.
    pub scale_factor: Option<f32>,
}

/// Flattened `sc_crafting::ValueRange` — one quality→value band, linearly
/// interpolated and clamped to `[start_quality, end_quality]`. `additive`
/// distinguishes a multiplicative band (`Linear`, the common case — `×factor`)
/// from an additive one (`LinearIntegerAdditive` — `+value`). Additive ints
/// are widened to `f32`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct ModifierRange {
    pub additive: bool,
    pub start_quality: i32,
    pub end_quality: i32,
    pub at_start: f32,
    pub at_end: f32,
}

/// Where an inventory stack physically sits, classified from sc-dossier's
/// `Context`. `Location` / `Hangar` carry a resolved place name in
/// [`InventoryStack::location_name`]; `Container` carries the owning ship/box
/// geid in [`InventoryStack::container_geid`] (a live instance id, not
/// holotable-resolvable to a name). Storage form is the lowercase string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum InventoryLocationKind {
    /// On the character (`PlayerInventory`).
    Player,
    /// A world location (the `location_name` is the resolved place).
    Location,
    /// A hangar (the `location_name` is the resolved place).
    Hangar,
    /// Inside a ship/container (the `container_geid` is its instance id).
    Container,
    /// Bound to an entitlement.
    Entitlement,
    /// Any other / future context kind.
    Other,
}

impl InventoryLocationKind {
    /// Storage representation — also the serde rename.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Player => "player",
            Self::Location => "location",
            Self::Hangar => "hangar",
            Self::Container => "container",
            Self::Entitlement => "entitlement",
            Self::Other => "other",
        }
    }

    /// Parse from the storage form. `Option` (not `FromStr`) so an unknown
    /// value is a soft failure at the row-mapping boundary.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "player" => Some(Self::Player),
            "location" => Some(Self::Location),
            "hangar" => Some(Self::Hangar),
            "container" => Some(Self::Container),
            "entitlement" => Some(Self::Entitlement),
            "other" => Some(Self::Other),
            _ => None,
        }
    }
}

/// One stack in the player's live inventory, resolved from sc-dossier's wire
/// data against the catalog at sync time. Scoped by `(account_id, platform)`
/// like every personal-state row, and replaced wholesale on each authoritative
/// sync (it's a snapshot, not an incremental ledger).
///
/// [`Self::kind`] discriminates the two shapes — the same split recipes use:
/// a [`IngredientKind::Resource`] stack carries [`Self::scu`] + [`Self::quality`];
/// a [`IngredientKind::Item`] stack carries [`Self::count`]. [`Self::crc`] is the
/// match key — `resource_id` for resources, `class_crc` for items — so a recipe
/// ingredient ([`Ingredient::crc`]) lines up against it directly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct InventoryStack {
    pub id: RecordId,
    /// `resource_id` (resource stacks) or `class_crc` (item stacks) — the
    /// EntityGraph wire id this stack is keyed on. Matches [`Ingredient::crc`].
    pub crc: u32,
    /// Whether this is a bulk resource stack or a discrete item stack.
    pub kind: IngredientKind,
    /// Resolved display name (e.g. `"Aluminum"`, `"Hadanite"`). `None` when the
    /// CRC didn't resolve against the catalog at sync time.
    pub name: Option<String>,
    /// Material quality `0..=1000`, for `Resource` stacks. `None` for items.
    pub quality: Option<u16>,
    /// Quantity in SCU, for `Resource` stacks. `None` for items.
    pub scu: Option<f32>,
    /// Discrete unit count, for `Item` stacks. `None` for resources.
    pub count: Option<i32>,
    /// Where this stack sits.
    pub location_kind: InventoryLocationKind,
    /// Resolved place name for `Location` / `Hangar` contexts. `None` otherwise
    /// or when the place CRC didn't resolve.
    pub location_name: Option<String>,
    /// Owning ship/container geid (decimal string) for `Container` contexts.
    /// `None` otherwise. String, not u64, to stay IPC/serde-friendly.
    pub container_geid: Option<String>,
    pub platform: Platform,
    /// FK to `accounts.id`.
    pub account_id: RecordId,
    /// When this snapshot was synced.
    pub synced_at: DateTime<Utc>,
}
