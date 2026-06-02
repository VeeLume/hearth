//! Loads SC reference data from the local game install in two stages
//! along the natural fast/slow seam, with a layered snapshot cache for
//! fast subsequent loads.
//!
//! # Two stages, separated so the UI can show what's ready
//!
//! - [`discover`] — **fast** (~50ms): launcher-store reads. Finds the
//!   highest-priority install, derives the [`Platform`], reads the RSI
//!   handle. Everything the sidebar's scope chip needs to render. Wraps
//!   the synchronous `sc_holotable::install::*` calls in `spawn_blocking`
//!   so they don't stall the tokio runtime.
//! - [`build_catalog`] — **slow** (0.15s warm → ~30s cold): the DCB-parse
//!   waterfall described below. Returns the cooked `Vec<BpView>` for the
//!   catalog UI. Takes the [`Installation`] from `discover` so it doesn't
//!   redo discovery.
//!
//! AppState wires both as independent OnceCells, so callers wait only on
//! the data they actually need: `list_owned` and `active_scope` get
//! through in ~50ms; only `list_blueprints` waits on the catalog.
//!
//! # Catalog load waterfall (inside `build_catalog`)
//!
//! Each tier produces a `Vec<BpView>`; on success the cold tiers also
//! persist a cache file the next session can load instead.
//!
//! 1. **Processed snapshot** (`catalog.cook`) — load the cooked
//!    `Vec<BpView>` directly via [`ProcessedSnapshot`]. Sub-second. No
//!    parsing. Invalidated by a [`HEARTH_CATALOG_COOK_VERSION`] bump (the
//!    cooked layout changed) or an SC patch (`meta.build_id` mismatch).
//! 2. **Raw extract snapshot** (`extract.snap`) — load captured DCB +
//!    `global.ini` bytes via [`ExtractSnapshot`], re-parse to a live
//!    [`Datacore`], build the catalog, persist a fresh `catalog.cook`.
//!    Skips p4k extraction; still pays the DCB-parse cost. Invalidated
//!    by sc-extract's [`ExtractSnapshot::SCHEMA_VERSION`] bump or an SC
//!    patch.
//! 3. **Live parse** — open the live `Data.p4k`, extract assets, parse
//!    the DCB, build the catalog. Persist both `extract.snap` and
//!    `catalog.cook` for next time. The cold path; the slow one.
//! 4. **Error** — live parse failed; bubble up.
//!
//! Snapshot failures (missing file, version mismatch, staleness, decode
//! error) are non-fatal: they log at info level and fall through to the
//! next tier. Only a full live-parse failure propagates.
//!
//! Cache files live under `%APPDATA%/hearth/cache/<channel>/` (one
//! directory per channel; the build_id check inside the snapshot handles
//! patches within a channel). Atomic writes mean a crash mid-save can't
//! leave a half-written file behind.
//!
//! # Stack-size workaround (Windows)
//!
//! sc-extract-generated's `record_store` decoder has match arms deep
//! enough to overflow the default thread stack on Windows. Pattern
//! lifted from sc-langpatch / bulkhead:
//!
//! 1. Run the catalog build on a dedicated `std::thread` with an
//!    explicit 32 MiB stack (matches bulkhead's `LOADER_STACK_SIZE`).
//! 2. Return only `Vec<BpView>` through the bridge — owned data, no
//!    boxing tricks needed (the heavy `Datacore` lives only inside the
//!    loader thread and gets dropped before sending).
//!
//! `discover` does *not* need the big stack — it never touches the DCB.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use hearth_core::sc_data::guid_string;
use hearth_core::{
    BpPoolReward, BpRewardEntry, BpView, ItemRewardView, MissionView, Platform, RepRewardView,
    ScripRewardView,
};
use sc_holotable::asset::{
    AssetConfig, AssetData, AssetSource, Datacore, ExtractSnapshot, LocaleKey, LocaleMap,
    ProcessedSnapshot, RecordPaths, SnapshotCaptureConfig, snapshot_meta_from_install,
};
use sc_holotable::crafting::{Blueprints, Categories, Process};
use sc_holotable::install::{Channel, Installation};
use sc_holotable::items::{ItemCatalog, Items};
use sc_holotable::missions::{Mission, Missions, RewardAmount, RewardCurrencies};
use sc_holotable::resources::Resources;

pub const LOADER_STACK_SIZE: usize = 32 * 1024 * 1024;

/// Cook-format version for hearth's processed snapshot. Bump whenever the
/// cooked [`CookedData`] serde shape changes ([`BpView`] or [`MissionView`]
/// fields added/renamed/retyped) so older caches invalidate cleanly via
/// `Error::ProcessedSnapshotStale` instead of deserializing into a
/// silently-wrong shape. (17: mission pool key now includes encounter shape
/// (difficulty tiers split); diagnostic removed.)
const HEARTH_CATALOG_COOK_VERSION: u32 = 17;

pub const EXTRACT_SNAPSHOT_NAME: &str = "extract.snap";
pub const CATALOG_SNAPSHOT_NAME: &str = "catalog.cook";

/// The cooked SC reference data hearth caches per channel — the blueprint
/// catalog and the mission browser data, both built from one `Datacore`
/// parse so the cold path pays the DCB cost once. Serialized whole into the
/// processed snapshot (`catalog.cook`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CookedData {
    pub blueprints: Vec<BpView>,
    pub missions: Vec<MissionView>,
}

/// Everything the sidebar / scope chip / DB-only commands need before
/// the heavy DCB parse starts. Produced by [`discover`] in ~50ms.
#[derive(Debug, Clone)]
pub struct Discovery {
    /// Specific channel that produced this dataset (Live, Hotfix, …).
    pub channel: Channel,
    /// Stability grouping. `Prod` (Live + Hotfix) or `Ptu` (PTU, EPTU,
    /// TechPreview). Read from launcher store when available; falls
    /// back to a channel-based map otherwise.
    pub platform: Platform,
    /// RSI handle from the launcher store, if available. `None` if the
    /// launcher store couldn't be read or the identity block was empty
    /// (e.g. user has never signed into the launcher).
    pub handle: Option<String>,
    /// The chosen install. Held so [`build_catalog`] doesn't redo
    /// discovery, and so future stages (sensors / Game.log tailing) can
    /// resolve paths off it.
    pub install: Installation,
}

/// Find the highest-priority install, derive its [`Platform`], read the
/// launcher-store identity. Runs the synchronous launcher reads via
/// `spawn_blocking` so the tokio runtime stays responsive.
pub async fn discover() -> Result<Discovery> {
    tokio::task::spawn_blocking(discover_blocking)
        .await
        .context("joining discovery task")?
}

fn discover_blocking() -> Result<Discovery> {
    let start = Instant::now();
    let mut installs = sc_holotable::install::discover().context("sc discovery")?;
    if installs.is_empty() {
        anyhow::bail!("no Star Citizen installations detected");
    }
    installs.sort_by_key(|i| i.channel.priority());
    let install = installs.into_iter().next().expect("non-empty");
    let channel = install.channel;
    let platform = install
        .platform_id
        .as_deref()
        .and_then(Platform::from_str)
        .unwrap_or_else(|| group_for(channel));

    // Best-effort handle read. Failure here is logged but not fatal —
    // identity is bootstrapped from the launcher store when available;
    // sign-in flow / manual entry fills the gap otherwise.
    let handle = match sc_holotable::install::read_identity() {
        Ok(id) => Some(id.handle),
        Err(e) => {
            tracing::info!("launcher identity unavailable ({e}); handle stays unbound");
            None
        }
    };

    tracing::info!(
        channel = ?channel,
        platform = platform.as_str(),
        handle = ?handle,
        elapsed_ms = start.elapsed().as_millis(),
        "discovery complete"
    );
    Ok(Discovery {
        channel,
        platform,
        handle,
        install,
    })
}

/// Build the cooked SC reference data (catalog + missions) for the given
/// install, running on a dedicated 32 MiB-stack thread (see module docs for
/// the stack rationale). Runs the snapshot waterfall internally.
pub async fn build_data(install: Installation) -> Result<CookedData> {
    let (tx, rx) = mpsc::channel::<Result<CookedData>>();
    std::thread::Builder::new()
        .name("hearth-catalog-loader".into())
        .stack_size(LOADER_STACK_SIZE)
        .spawn(move || {
            let _ = tx.send(build_data_blocking(install));
        })
        .context("spawning catalog-loader thread")?;
    tokio::task::spawn_blocking(move || rx.recv())
        .await
        .context("joining catalog-loader bridge task")?
        .map_err(|_| anyhow!("catalog-loader sender dropped"))?
}

fn build_data_blocking(install: Installation) -> Result<CookedData> {
    let start = Instant::now();
    let channel = install.channel;
    let cache_dir = cache_dir_for(channel)?;

    // ── Tier 1: processed snapshot (sub-second) ───────────────────
    if let Some(cooked) = try_load_processed(&cache_dir, &install) {
        tracing::info!(
            blueprints = cooked.blueprints.len(),
            missions = cooked.missions.len(),
            channel = ?channel,
            elapsed_ms = start.elapsed().as_millis(),
            "data loaded from processed snapshot"
        );
        return Ok(cooked);
    }

    // ── Tier 2: raw extract snapshot (skip p4k extraction) ────────
    if let Some((datacore, locale)) = try_load_extract(&cache_dir, &install) {
        let cooked = build_cooked(&datacore, &locale);
        save_processed(&cache_dir, &install, &cooked);
        tracing::info!(
            blueprints = cooked.blueprints.len(),
            missions = cooked.missions.len(),
            channel = ?channel,
            elapsed_ms = start.elapsed().as_millis(),
            "data built from raw extract snapshot"
        );
        return Ok(cooked);
    }

    // ── Tier 3: live parse (cold path) ────────────────────────────
    tracing::info!(channel = ?channel, "no usable snapshot; parsing live Data.p4k");
    let p4k_path = install.data_p4k();
    let assets = AssetSource::open(&p4k_path)
        .with_context(|| format!("opening Data.p4k for {channel:?}"))?;

    tracing::info!("extracting AssetData ({:?})", channel);
    let asset_data =
        AssetData::extract(&assets, &AssetConfig::minimal()).context("AssetData::extract")?;
    tracing::info!("parsing Datacore ({:?})", channel);
    let datacore = Datacore::parse(&assets, &asset_data).context("Datacore::parse")?;

    // Capture the raw bytes while `assets` is still open; failure to
    // persist the cache is non-fatal (logged inside).
    save_extract(&cache_dir, &install, &assets);

    let locale_bytes = read_locale_bytes(&assets).context("reading global.ini from Data.p4k")?;
    drop(assets);
    let locale = build_locale_map(&locale_bytes)?;

    tracing::info!("building catalog + missions");
    let cooked = build_cooked(&datacore, &locale);
    save_processed(&cache_dir, &install, &cooked);

    tracing::info!(
        blueprints = cooked.blueprints.len(),
        missions = cooked.missions.len(),
        channel = ?channel,
        elapsed_ms = start.elapsed().as_millis(),
        "data built from live parse"
    );
    Ok(cooked)
}

/// Cook both products from one parsed `Datacore`. Each builder makes its own
/// indices (cheap relative to the DCB parse this shares), so they stay
/// self-contained.
fn build_cooked(datacore: &Datacore, locale: &LocaleMap) -> CookedData {
    CookedData {
        blueprints: build_blueprints(datacore, locale),
        missions: build_missions(datacore, locale),
    }
}

// ── Cache helpers ──────────────────────────────────────────────────────

/// Per-channel cache directory under Hearth's data root
/// (`%APPDATA%/hearth[-dev]/cache/<channel>/` on Windows). Honours the
/// dev/release namespace split via [`crate::app_data_root`].
fn cache_dir_for(channel: Channel) -> Result<PathBuf> {
    let key = channel.install_dir_name().to_ascii_lowercase();
    Ok(crate::app_data_root().join("cache").join(key))
}

/// Which cache tier the next catalog load will *likely* use, for the loading
/// message. Predicted from snapshot-file existence (cheap, no parse) — not a
/// guarantee: a stale snapshot after an SC patch still falls through to a
/// slower tier, which is why the UI pairs it with a "may take longer" note.
#[derive(Debug, Clone, Copy, serde::Serialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum LoadTier {
    /// Processed snapshot present — sub-second.
    Processed,
    /// Only the raw extract snapshot present — re-parse, medium.
    Cache,
    /// No snapshot — live `Data.p4k` parse, slow.
    Raw,
}

/// Predict the load tier for `channel` by which snapshot files exist on disk.
pub fn predict_tier(channel: Channel) -> LoadTier {
    let Ok(dir) = cache_dir_for(channel) else {
        return LoadTier::Raw;
    };
    if dir.join(CATALOG_SNAPSHOT_NAME).exists() {
        LoadTier::Processed
    } else if dir.join(EXTRACT_SNAPSHOT_NAME).exists() {
        LoadTier::Cache
    } else {
        LoadTier::Raw
    }
}

/// Try to load the cooked catalog directly. `None` on any failure — the
/// caller falls through to the next tier. The build_id staleness check
/// catches SC patches that happened since the snapshot was written.
fn try_load_processed(cache_dir: &Path, install: &Installation) -> Option<CookedData> {
    let path = cache_dir.join(CATALOG_SNAPSHOT_NAME);
    if !path.exists() {
        return None;
    }
    let snap = match ProcessedSnapshot::<CookedData>::load(&path, HEARTH_CATALOG_COOK_VERSION) {
        Ok(s) => s,
        Err(e) => {
            tracing::info!("processed snapshot unusable ({e}); falling back");
            return None;
        }
    };
    if snap.meta.build_id != install.manifest.build_id {
        tracing::info!(
            snapshot_build_id = %snap.meta.build_id,
            install_build_id = %install.manifest.build_id,
            "processed snapshot stale (SC patched); falling back"
        );
        return None;
    }
    Some(snap.into_index())
}

fn save_processed(cache_dir: &Path, install: &Installation, cooked: &CookedData) {
    let path = cache_dir.join(CATALOG_SNAPSHOT_NAME);
    let meta = snapshot_meta_from_install(install);
    let snap = ProcessedSnapshot::new(meta, HEARTH_CATALOG_COOK_VERSION, cooked.clone());
    if let Err(e) = snap.save(&path) {
        tracing::warn!(
            "failed to save processed snapshot to {}: {e}",
            path.display()
        );
    } else {
        tracing::debug!("wrote processed snapshot to {}", path.display());
    }
}

/// Try to hydrate the raw extract snapshot into a live `Datacore` +
/// `LocaleMap`. `None` on any failure. Skips p4k extraction entirely; still
/// pays the DCB-parse cost, but avoids the zstd-decompression-of-p4k cost.
fn try_load_extract(cache_dir: &Path, install: &Installation) -> Option<(Datacore, LocaleMap)> {
    let path = cache_dir.join(EXTRACT_SNAPSHOT_NAME);
    if !path.exists() {
        return None;
    }
    let snap = match ExtractSnapshot::load(&path) {
        Ok(s) => s,
        Err(e) => {
            tracing::info!("extract snapshot unusable ({e}); falling back to live parse");
            return None;
        }
    };
    if snap.meta.build_id != install.manifest.build_id {
        tracing::info!(
            snapshot_build_id = %snap.meta.build_id,
            install_build_id = %install.manifest.build_id,
            "extract snapshot stale (SC patched); falling back to live parse"
        );
        return None;
    }

    // Replicate ExtractSnapshot::hydrate manually so we keep the memory-
    // backed `source` around to read global.ini bytes from it after
    // parsing (hydrate consumes its source internally).
    let label = format!("snapshot://{}", snap.meta.build_id);
    let source = AssetSource::from_snapshot(snap.files.clone(), label);
    let asset_data = match AssetData::extract(&source, &AssetConfig::minimal()) {
        Ok(d) => d,
        Err(e) => {
            tracing::info!("hydrate failed at AssetData::extract ({e}); falling back");
            return None;
        }
    };
    let datacore = match Datacore::parse(&source, &asset_data) {
        Ok(d) => d,
        Err(e) => {
            tracing::info!("hydrate failed at Datacore::parse ({e}); falling back");
            return None;
        }
    };
    let locale_bytes = match read_locale_bytes(&source) {
        Ok(b) => b,
        Err(e) => {
            tracing::info!("hydrate failed reading global.ini ({e}); falling back");
            return None;
        }
    };
    let locale = match build_locale_map(&locale_bytes) {
        Ok(l) => l,
        Err(e) => {
            tracing::info!("hydrate failed building locale map ({e}); falling back");
            return None;
        }
    };
    Some((datacore, locale))
}

fn save_extract(cache_dir: &Path, install: &Installation, assets: &AssetSource) {
    let path = cache_dir.join(EXTRACT_SNAPSHOT_NAME);
    let meta = snapshot_meta_from_install(install);
    let snap = match ExtractSnapshot::capture(assets, meta, &SnapshotCaptureConfig::standard()) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("failed to capture extract snapshot: {e}");
            return;
        }
    };
    if let Err(e) = snap.save(&path) {
        tracing::warn!("failed to save extract snapshot to {}: {e}", path.display());
    } else {
        tracing::debug!("wrote extract snapshot to {}", path.display());
    }
}

/// Find `global.ini` in either a live p4k or a memory-backed (snapshot)
/// source. Predicate accepts both separator forms because captured
/// snapshot keys carry the archive's backslash separator while live p4k
/// path-matching is case-insensitive but separator-sensitive (see
/// AssetSource::read).
fn read_locale_bytes(source: &AssetSource) -> Result<Vec<u8>> {
    let (_, bytes) = source
        .find_and_read(|name| {
            let n = name.to_ascii_lowercase();
            n.ends_with("english\\global.ini") || n.ends_with("english/global.ini")
        })
        .context("searching for english/global.ini")?
        .ok_or_else(|| anyhow!("english/global.ini not present in archive/snapshot"))?;
    Ok(bytes)
}

// ── Build helpers ──────────────────────────────────────────────────────

/// Channel-based fallback when the launcher store didn't give us a
/// `platform_id` (e.g. log-fallback discovery). Mirrors CIG's own
/// platform mapping: Live + Hotfix → prod; everything else → ptu.
fn group_for(channel: Channel) -> Platform {
    match channel {
        Channel::Live | Channel::Hotfix => Platform::Prod,
        Channel::Ptu | Channel::Eptu | Channel::TechPreview => Platform::Ptu,
    }
}

fn build_blueprints(datacore: &Datacore, locale: &LocaleMap) -> Vec<BpView> {
    // Index passes over the same datacore:
    //   - Items        — entity name keys + typed Type/SubType
    //   - Blueprints   — the full crafting catalog (Blueprint + tier-0 Recipe)
    //   - Resources    — name_key + density for each ResourceType
    //   - RecordPaths  — Categories::build needs the path/name lookup
    //                    (categories are empty marker records — identity
    //                    is the record's name, exposed via RecordPaths)
    //   - Categories   — CIG-authored crafting taxonomy (20 entries in
    //                    SC 4.8: FPSWeapons, FPSArmours, Medical,
    //                    VehicleWeaponsS1-6, ...)
    //   - ItemCatalog  — variant grouping (paint / skin / "Modified"
    //                    variants of one model share a model id)
    // Items is the only index that's also passed downstream (Blueprints
    // and ItemCatalog both need it).
    let items = Items::build(datacore.records());
    let catalog = Blueprints::build(datacore, &items);
    let resources = Resources::build(datacore.records());
    let paths = RecordPaths::build(datacore);
    let categories = Categories::build(&paths);
    // ItemCatalog clusters gear into Models (one design + one slot: an
    // item and its colorway variants) grouped under Collections (the
    // models that read as the same design across slots/accessories, e.g.
    // "Geist Armor"). Grouping is display-name driven (Items + &LocaleMap);
    // &RecordPaths is used to pick the canonical base member of each model.
    let item_catalog = ItemCatalog::build(&items, &paths, locale);

    let mut out = Vec::new();
    for blueprint in catalog.iter() {
        // Filter to Creation blueprints — the ones that craft a real
        // entity. Dormant non-Creation processes (refining/repair/etc
        // — all 0 records in SC 4.8 but the schema reserves them)
        // would surface here with no crafted_entity_guid; skip them.
        if !matches!(blueprint.process, Process::Creation { .. }) {
            continue;
        }
        let mut view = hearth_core::sc_data::bp_view(blueprint);
        view.display_name = blueprint.display_name(locale).map(|s| s.to_owned());
        if let Some(entity_guid) = blueprint.crafted_entity_guid() {
            // item_type/item_sub_type are typed enums; the IPC boundary
            // carries their DCB-string form (what itemTypes.ts keys on).
            view.item_type = items
                .item_type(&entity_guid)
                .map(|t| t.as_dcb_str().to_owned());
            view.item_sub_type = items
                .item_sub_type(&entity_guid)
                .map(|t| t.as_dcb_str().to_owned());
        }
        // sc-crafting category — CIG-authored grouping. Strip the
        // verbose record-class prefix so the IPC carries just the
        // semantic name ("FPSArmours", "VehicleWeaponsS3", ...).
        if let Some(cat_guid) = blueprint.category
            && let Some(cat) = categories.get(&cat_guid)
        {
            view.category_raw = Some(
                cat.name
                    .strip_prefix("BlueprintCategoryRecord.")
                    .unwrap_or(&cat.name)
                    .to_owned(),
            );
        }
        // Variant-bundling key — ItemCatalog returns Some for any gear
        // item it grouped into a model (the solo fallback covers items
        // with no other signal), so this falls back to the raw guid when
        // the entity isn't gear (ship components, props) or the BP has
        // no crafted entity at all.
        if let Some(entity_guid) = blueprint.crafted_entity_guid() {
            view.family_id = Some(
                item_catalog
                    .model_id_of(&entity_guid)
                    .map(str::to_owned)
                    .unwrap_or_else(|| entity_guid.to_string()),
            );

            // Model base name — resolved through Items + LocaleMap so
            // the catalog UI's bundle row header reads the canonical
            // base item's name even when only variants are blueprinted.
            // All members of one model resolve to the same base name
            // here; the small redundant work per-BP is cheaper than
            // restructuring the loop to pre-compute per-model.
            if let Some(base_guid) = item_catalog.base_of(&entity_guid)
                && let Some(name_key) = items.name_key(&base_guid)
                && let Some(name) = locale.resolve(name_key)
                && !name.is_empty()
            {
                view.family_base_name = Some(name.to_owned());
            }
        }
        // Resolve resource_name on each ingredient (bp_view leaves it
        // None because it has no Resources/LocaleMap access).
        if let Some(recipe) = view.recipe.as_mut() {
            fill_resource_names(&mut recipe.ingredients, &resources, locale);
        }
        out.push(view);
    }
    out
}

/// Build the mission browser data. CIG spawns one contract per offered
/// locality, so the raw list is thousands of near-duplicates; we **pool**
/// contracts that share a `(title_key, description_key)` into one template
/// (the mission a player perceives) and aggregate their localities into
/// `regions`. Per template we surface the reward axes (aUEC, scrip,
/// reputation, item unlocks, blueprint pools — the last resolved through
/// `Missions::blueprints`), a one-line encounter banner, and the count of
/// pooled instances.
fn build_missions(datacore: &Datacore, locale: &LocaleMap) -> Vec<MissionView> {
    let items = Items::build(datacore.records());
    let currencies = RewardCurrencies::build(datacore);
    let missions = Missions::build(datacore);
    let pools = &missions.blueprints;
    let localities = &missions.localities;

    // Group contracts into templates. Localities are *aggregated* (same
    // mission offered in many places = one row with a region list), not split
    // — splitting by location would re-introduce the duplication we set out to
    // remove. Templates are kept distinct by the axes that make missions
    // genuinely different:
    //   - reward identity (faction-specific pools, rep faction, scrip, …),
    //   - encounter shape (difficulty tiers — a 2-ship VeryEasy and a 4-ship
    //     Hard version of one contract are different missions).
    // Validated against SCMDB's blueprint-mission count via a diagnostic:
    // title+desc+reward = 459, +encounter = 483 (~87% of SCMDB's 558; the
    // rest is SCMDB splitting by location/standing, which we deliberately
    // don't).
    type PoolKey = (Option<LocaleKey>, Option<LocaleKey>, String, String);
    let mut groups: HashMap<PoolKey, Vec<&Mission>> = HashMap::new();
    for m in missions.iter() {
        let key = (
            m.title_key.clone(),
            m.description_key.clone(),
            reward_signature(m),
            encounter_summary(m).unwrap_or_default(),
        );
        groups.entry(key).or_default().push(m);
    }

    let mut out = Vec::with_capacity(groups.len());
    for members in groups.values() {
        // Members share title/description/rewards (same template); the first
        // is the representative. Localities are what vary → aggregated below.
        let rep = members[0];
        let r = &rep.rewards;

        let (uec_fixed, uec_calculated) = match r.uec {
            RewardAmount::Fixed(n) => (Some(n), false),
            RewardAmount::Calculated => (None, true),
            RewardAmount::None => (None, false),
        };
        let scrip = r
            .scrip
            .iter()
            .map(|s| ScripRewardView {
                name: currencies
                    .display_name(&s.currency_guid, &items, locale)
                    .map(str::to_owned),
                amount: s.amount,
            })
            .collect();
        let reputation = r
            .reputation
            .iter()
            .map(|rep| RepRewardView {
                faction_guid: rep.faction.as_ref().map(guid_string),
                amount: rep.amount,
            })
            .collect();
        let item_rewards = r
            .items
            .iter()
            .map(|it| ItemRewardView {
                entity_guid: guid_string(&it.entity_class),
                name: items
                    .name_key(&it.entity_class)
                    .and_then(|k| locale.resolve(k))
                    .map(str::to_owned),
                amount: it.amount,
            })
            .collect();

        // Blueprint rewards: union of distinct pools across members.
        let mut seen_pools = HashSet::new();
        let mut blueprint_rewards = Vec::new();
        for m in members {
            for br in &m.rewards.blueprints {
                if !seen_pools.insert(br.pool_guid) {
                    continue;
                }
                let Some(pool) = pools.get(&br.pool_guid) else {
                    continue;
                };
                let blueprints = pool
                    .items
                    .iter()
                    .map(|e| BpRewardEntry {
                        blueprint_record_guid: guid_string(&e.blueprint.blueprint_record_guid),
                        name: e.blueprint.display_name(locale).map(str::to_owned),
                        weight: e.weight,
                    })
                    .collect();
                blueprint_rewards.push(BpPoolReward {
                    pool_name: pool.name.clone(),
                    chance: br.chance,
                    blueprints,
                });
            }
        }

        // Regions: distinct locality labels across all pooled members.
        let mut seen_regions = HashSet::new();
        let mut regions = Vec::new();
        for m in members {
            for guid in &m.mission_span {
                if let Some(view) = localities.get(guid) {
                    let label = view.region_label(locale);
                    if !label.is_empty() && seen_regions.insert(label.clone()) {
                        regions.push(label);
                    }
                }
            }
        }
        regions.sort();

        out.push(MissionView {
            mission_id: guid_string(&rep.id),
            title: rep.title(locale).map(clean_mission_text),
            debug_name: rep.debug_name.clone(),
            description: rep.description(locale).map(clean_mission_text),
            once_only: rep.availability.once_only,
            shareable: rep.shareable,
            illegal: rep.illegal_flag,
            cooldown_seconds: rep
                .availability
                .cooldowns
                .completion
                .as_ref()
                .map(|d| d.mean_seconds),
            uec_fixed,
            uec_calculated,
            scrip,
            reputation,
            item_rewards,
            blueprint_rewards,
            regions,
            encounter_summary: encounter_summary(rep),
            instance_count: members.len() as u32,
        });
    }

    // Stable, readable order: by title (debug_name fallback), then id.
    out.sort_by(|a, b| {
        a.title
            .as_deref()
            .unwrap_or(&a.debug_name)
            .cmp(b.title.as_deref().unwrap_or(&b.debug_name))
            .then_with(|| a.mission_id.cmp(&b.mission_id))
    });
    out
}

/// Distinguishing reward identity for pooling. Two contracts sharing a
/// title+description but different rewards are different missions — most
/// tellingly the **reputation faction** (the giving faction), plus the BP
/// pools, scrip currencies, item unlocks, and any fixed aUEC. (Calculated
/// aUEC is engine-computed and not a stable splitter, so it's excluded.)
fn reward_signature(m: &Mission) -> String {
    let r = &m.rewards;
    let mut parts: Vec<String> = Vec::new();
    for br in &r.blueprints {
        parts.push(format!("b{}", guid_string(&br.pool_guid)));
    }
    for rep in &r.reputation {
        if let Some(f) = &rep.faction {
            parts.push(format!("r{}", guid_string(f)));
        }
    }
    for s in &r.scrip {
        parts.push(format!("s{}", guid_string(&s.currency_guid)));
    }
    for it in &r.items {
        parts.push(format!("i{}", guid_string(&it.entity_class)));
    }
    if let RewardAmount::Fixed(n) = r.uec {
        parts.push(format!("u{n}"));
    }
    parts.sort();
    parts.dedup();
    parts.join(",")
}

/// Render `~mission(Var)` runtime-substitution markers as readable `[Var]`
/// placeholders (the engine fills these per spawn; a static view can't).
fn clean_mission_text(s: &str) -> String {
    const MARK: &str = "~mission(";
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find(MARK) {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + MARK.len()..];
        match after.find(')') {
            Some(end) => {
                out.push('[');
                out.push_str(&after[..end]);
                out.push(']');
                rest = &after[end + 1..];
            }
            None => {
                // Unterminated marker — emit the remainder verbatim.
                out.push_str(&rest[pos..]);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

/// One-line encounter banner — `"2-4 ships · VeryEasy"`. `None` when the
/// mission has no ship/entity encounters and no combat class.
fn encounter_summary(m: &sc_holotable::missions::Mission) -> Option<String> {
    let (min, max) = m.ship_count_range();
    let class = m.combat_class();
    if max == 0 && class.is_none() {
        return None;
    }
    let mut parts = Vec::new();
    if max > 0 {
        let count = if min == max {
            max.to_string()
        } else {
            format!("{min}-{max}")
        };
        let noun = if max == 1 { "ship" } else { "ships" };
        parts.push(format!("{count} {noun}"));
    }
    if let Some(c) = class {
        parts.push(c.to_string());
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// Fill `Ingredient.resource_name` for each ingredient by looking up
/// the resource's `name_key` in the locale map. Ingredients whose GUID
/// doesn't parse or doesn't resolve stay `None`; the UI falls back to
/// the GUID.
fn fill_resource_names(
    ingredients: &mut [hearth_core::Ingredient],
    resources: &Resources,
    locale: &LocaleMap,
) {
    for ing in ingredients {
        let Ok(guid) = ing.resource_guid.parse::<sc_holotable::asset::Guid>() else {
            continue;
        };
        let Some(resource) = resources.get(&guid) else {
            continue;
        };
        if let Some(name) = locale.resolve(&resource.name_key)
            && !name.is_empty()
        {
            ing.resource_name = Some(name.to_owned());
        }
    }
}

fn build_locale_map(bytes: &[u8]) -> Result<LocaleMap> {
    let (decoded, _, had_errors) = encoding_rs::UTF_16LE.decode(bytes);
    if had_errors {
        anyhow::bail!("UTF-16 LE decoding produced errors on global.ini");
    }
    let content = decoded.into_owned();
    let content = content.strip_prefix('\u{FEFF}').unwrap_or(&content);

    let mut map = LocaleMap::new();
    for line in content.lines() {
        if let Some(eq_pos) = line.find('=') {
            let raw_key = &line[..eq_pos];
            let key = sc_holotable::asset::strip_locale_metadata(raw_key);
            let value = &line[eq_pos + 1..];
            map.set(key, value);
        }
    }
    Ok(map)
}
