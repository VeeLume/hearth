//! Loads SC reference data (the blueprint catalog) from the local game
//! install, with a layered snapshot cache for fast subsequent loads.
//!
//! # Load waterfall
//!
//! Each tier produces a `Vec<BpView>`; on success the cold tiers also
//! persist a cache file the next session can load instead. The trade-off
//! climbs: cheap-to-load + cheap-to-cache up top, expensive-to-build at
//! the bottom.
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
//! 4. **Error** — no usable install or live parse failed; bubble up.
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
//! # Channel + platform selection
//!
//! `sc_holotable::install::discover()` returns every installed channel
//! sorted by `Channel::priority()` (Live → Hotfix → PTU → EPTU →
//! TechPreview). We pick the first — the most-stable available install.
//!
//! The chosen install's `platform_id` (`'prod'` vs `'ptu'`) comes
//! straight from the launcher store (since sc-holotable v0.6) and gets
//! recorded on the loaded data so personal-state writes are scoped
//! correctly. If launcher-store fallback fired (log parsing),
//! `platform_id` is `None`; we derive it from the channel as a fallback.
//!
//! # Stack-size workaround (Windows)
//!
//! sc-extract-generated's `record_store` decoder has match arms deep
//! enough to overflow the default thread stack on Windows. Pattern
//! lifted from sc-langpatch / bulkhead:
//!
//! 1. Run `load_inner` on a dedicated `std::thread` with an explicit
//!    32 MiB stack (matches bulkhead's `LOADER_STACK_SIZE`).
//! 2. Return `Box<LoadedScData>` through the bridge — boxing on the
//!    loader thread means only an 8-byte pointer crosses the mpsc
//!    channel into the (small-stack) tokio blocking receiver.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use hearth_core::{BpView, Platform};
use sc_holotable::asset::{
    AssetConfig, AssetData, AssetSource, Datacore, ExtractSnapshot, LocaleMap, ProcessedSnapshot,
    SnapshotCaptureConfig, snapshot_meta_from_install,
};
use sc_holotable::install::{Channel, Installation};
use sc_holotable::items::Items;
use sc_holotable::missions::BlueprintPools;

pub const LOADER_STACK_SIZE: usize = 32 * 1024 * 1024;

/// Cook-format version for hearth's processed catalog snapshot. Bump
/// whenever [`BpView`]'s serde shape changes (added fields, renamed
/// fields, type changes) so older caches invalidate cleanly via
/// `Error::ProcessedSnapshotStale` instead of deserializing into a
/// silently-wrong shape.
const HEARTH_CATALOG_COOK_VERSION: u32 = 1;

const EXTRACT_SNAPSHOT_NAME: &str = "extract.snap";
const CATALOG_SNAPSHOT_NAME: &str = "catalog.cook";

pub struct LoadedScData {
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
    /// Pre-computed BP catalog.
    blueprints: Vec<BpView>,
}

impl LoadedScData {
    /// Spawn a dedicated 32 MiB-stack thread, load on it, return a
    /// `Box<Self>` via mpsc. See module docs for the stack rationale.
    pub async fn load_async() -> Result<Box<Self>> {
        let (tx, rx) = mpsc::channel::<Result<Box<Self>>>();
        std::thread::Builder::new()
            .name("hearth-sc-loader".into())
            .stack_size(LOADER_STACK_SIZE)
            .spawn(move || {
                let _ = tx.send(Self::load_inner().map(Box::new));
            })
            .context("spawning sc-loader thread")?;
        tokio::task::spawn_blocking(move || rx.recv())
            .await
            .context("joining sc-loader bridge task")?
            .map_err(|_| anyhow::anyhow!("sc-loader sender dropped"))?
    }

    pub fn load_inner() -> Result<Self> {
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

        let cache_dir = cache_dir_for(channel)?;

        // ── Tier 1: processed snapshot (sub-second) ───────────────────
        if let Some(blueprints) = try_load_processed(&cache_dir, &install) {
            tracing::info!(
                count = blueprints.len(),
                channel = ?channel,
                platform = platform.as_str(),
                elapsed_ms = start.elapsed().as_millis(),
                "catalog loaded from processed snapshot"
            );
            return Ok(Self { channel, platform, handle, blueprints });
        }

        // ── Tier 2: raw extract snapshot (skip p4k extraction) ────────
        if let Some((datacore, locale)) = try_load_extract(&cache_dir, &install) {
            let blueprints = build_blueprints(&datacore, &locale);
            save_processed(&cache_dir, &install, &blueprints);
            tracing::info!(
                count = blueprints.len(),
                channel = ?channel,
                platform = platform.as_str(),
                elapsed_ms = start.elapsed().as_millis(),
                "catalog built from raw extract snapshot"
            );
            return Ok(Self { channel, platform, handle, blueprints });
        }

        // ── Tier 3: live parse (cold path) ────────────────────────────
        tracing::info!(channel = ?channel, "no usable snapshot; parsing live Data.p4k");
        let p4k_path = install.data_p4k();
        let assets = AssetSource::open(&p4k_path)
            .with_context(|| format!("opening Data.p4k for {channel:?}"))?;

        tracing::info!("extracting AssetData ({:?})", channel);
        let asset_data = AssetData::extract(&assets, &AssetConfig::minimal())
            .context("AssetData::extract")?;
        tracing::info!("parsing Datacore ({:?})", channel);
        let datacore = Datacore::parse(&assets, &asset_data).context("Datacore::parse")?;

        // Capture the raw bytes while `assets` is still open; failure to
        // persist the cache is non-fatal (logged inside).
        save_extract(&cache_dir, &install, &assets);

        let locale_bytes =
            read_locale_bytes(&assets).context("reading global.ini from Data.p4k")?;
        drop(assets);
        let locale = build_locale_map(&locale_bytes)?;

        tracing::info!("building blueprint catalog");
        let blueprints = build_blueprints(&datacore, &locale);
        save_processed(&cache_dir, &install, &blueprints);

        tracing::info!(
            count = blueprints.len(),
            channel = ?channel,
            platform = platform.as_str(),
            elapsed_ms = start.elapsed().as_millis(),
            "catalog built from live parse"
        );
        Ok(Self { channel, platform, handle, blueprints })
    }

    pub fn blueprints(&self) -> Vec<BpView> {
        self.blueprints.clone()
    }
}

// ── Cache helpers ──────────────────────────────────────────────────────

/// Per-channel cache directory under the platform's user-data dir.
/// `%APPDATA%/hearth/cache/<channel>/` on Windows.
fn cache_dir_for(channel: Channel) -> Result<PathBuf> {
    let base = dirs::data_dir().ok_or_else(|| anyhow!("no platform data dir"))?;
    let key = channel.install_dir_name().to_ascii_lowercase();
    Ok(base.join("hearth").join("cache").join(key))
}

/// Try to load the cooked catalog directly. `None` on any failure — the
/// caller falls through to the next tier. The build_id staleness check
/// catches SC patches that happened since the snapshot was written.
fn try_load_processed(cache_dir: &Path, install: &Installation) -> Option<Vec<BpView>> {
    let path = cache_dir.join(CATALOG_SNAPSHOT_NAME);
    if !path.exists() {
        return None;
    }
    let snap = match ProcessedSnapshot::<Vec<BpView>>::load(&path, HEARTH_CATALOG_COOK_VERSION) {
        Ok(s) => s,
        Err(e) => {
            tracing::info!("processed catalog snapshot unusable ({e}); falling back");
            return None;
        }
    };
    if snap.meta.build_id != install.manifest.build_id {
        tracing::info!(
            snapshot_build_id = %snap.meta.build_id,
            install_build_id = %install.manifest.build_id,
            "processed catalog snapshot stale (SC patched); falling back"
        );
        return None;
    }
    Some(snap.into_index())
}

fn save_processed(cache_dir: &Path, install: &Installation, blueprints: &[BpView]) {
    let path = cache_dir.join(CATALOG_SNAPSHOT_NAME);
    let meta = snapshot_meta_from_install(install);
    let snap = ProcessedSnapshot::new(meta, HEARTH_CATALOG_COOK_VERSION, blueprints.to_vec());
    if let Err(e) = snap.save(&path) {
        tracing::warn!("failed to save processed catalog snapshot to {}: {e}", path.display());
    } else {
        tracing::debug!("wrote processed catalog snapshot to {}", path.display());
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
    // The per-entity item index (display-name keys + typed Type/SubType).
    // Since v0.8 this is an explicit build over the record store rather
    // than a field on the (now-removed) datacore snapshot.
    let items = Items::build(datacore.records());
    // Reward pools, resolved against the sc-crafting blueprint catalog.
    let pools = BlueprintPools::build(datacore, &items);

    // A blueprint crafts exactly one item, but the same blueprint can
    // appear in several reward pools — so iterating pools yields duplicate
    // entries per blueprint_record_guid. The catalog wants each blueprint
    // once; dedup by record GUID, keeping the first sighting.
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for pool in pools.iter() {
        for entry in &pool.items {
            let mut view = hearth_core::sc_data::bp_view(entry, pool);
            if !seen.insert(view.blueprint_record_guid.clone()) {
                continue;
            }
            view.display_name = entry.blueprint.display_name(locale).map(|s| s.to_owned());
            if let Some(entity_guid) = entry.blueprint.crafted_entity_guid() {
                // item_type/item_sub_type are now typed enums; the IPC
                // boundary carries their DCB-string form (what itemTypes.ts
                // already keys on).
                view.item_type = items.item_type(&entity_guid).map(|t| t.as_dcb_str().to_owned());
                view.item_sub_type = items
                    .item_sub_type(&entity_guid)
                    .map(|t| t.as_dcb_str().to_owned());
            }
            out.push(view);
        }
    }
    out
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
