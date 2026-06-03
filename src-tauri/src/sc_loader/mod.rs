//! Loads SC reference data from the local game install in two stages
//! along the natural fast/slow seam, with a layered snapshot cache for
//! fast subsequent loads.
//!
//! # Two stages, separated so the UI can show what's ready
//!
//! - [`discover`] — **fast** (~50ms): launcher-store reads. Finds the
//!   highest-priority install, derives the [`hearth_core::Platform`], reads
//!   the RSI handle. Everything the sidebar's scope chip needs to render.
//!   Lives in [`discover`].
//! - [`build_data`] — **slow** (0.15s warm → ~30s cold): the DCB-parse
//!   waterfall described below. Returns the cooked [`CookedData`] (catalog +
//!   missions). Takes the [`Installation`] from `discover` so it doesn't redo
//!   discovery.
//!
//! AppState wires both as independent OnceCells, so callers wait only on
//! the data they actually need: `list_owned` and `active_scope` get
//! through in ~50ms; only `list_blueprints` / `list_missions` wait on the
//! cooked data.
//!
//! # Module layout
//!
//! - [`discover`] — install discovery + identity (the fast stage).
//! - [`cache`] — the layered snapshot cache (`catalog.cook` / `extract.snap`)
//!   and the `global.ini` locale decode. The tier predicates ([`LoadTier`] /
//!   [`predict_tier`]) live here too.
//! - [`cook`] — turning a parsed `Datacore` into the catalog + mission views.
//! - this root — the [`build_data`] orchestration that runs the waterfall
//!   across those pieces.
//!
//! # Catalog load waterfall (inside [`build_data`])
//!
//! Each tier produces a [`CookedData`]; on success the cold tiers also
//! persist a cache file the next session can load instead.
//!
//! 1. **Processed snapshot** (`catalog.cook`) — load the cooked data directly.
//!    Sub-second. No parsing.
//! 2. **Raw extract snapshot** (`extract.snap`) — load captured DCB +
//!    `global.ini` bytes, re-parse to a live `Datacore`, cook, persist a fresh
//!    `catalog.cook`. Skips p4k extraction; still pays the DCB-parse cost.
//! 3. **Live parse** — open the live `Data.p4k`, extract assets, parse the
//!    DCB, cook. Persist both `extract.snap` and `catalog.cook`. The cold,
//!    slow path.
//! 4. **Error** — live parse failed; bubble up.
//!
//! Snapshot failures (missing file, version mismatch, staleness, decode
//! error) are non-fatal: they log at info level and fall through to the
//! next tier (see [`cache`]). Only a full live-parse failure propagates.
//!
//! # Stack-size workaround (Windows)
//!
//! sc-extract-generated's `record_store` decoder has match arms deep
//! enough to overflow the default thread stack on Windows. Pattern
//! lifted from sc-langpatch / bulkhead:
//!
//! 1. Run the catalog build on a dedicated `std::thread` with an
//!    explicit 32 MiB stack ([`LOADER_STACK_SIZE`]).
//! 2. Return only the owned [`CookedData`] through the bridge — no boxing
//!    tricks needed (the heavy `Datacore` lives only inside the loader thread
//!    and gets dropped before sending).
//!
//! `discover` does *not* need the big stack — it never touches the DCB.

pub mod cache;
pub mod cook;
pub mod discover;

pub use cache::{LoadTier, predict_tier};
pub use discover::{Discovery, discover};

use std::sync::mpsc;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use sc_holotable::asset::{AssetConfig, AssetData, AssetSource, Datacore};
use sc_holotable::install::Installation;

pub const LOADER_STACK_SIZE: usize = 32 * 1024 * 1024;

/// The cooked SC reference data hearth caches per channel — the blueprint
/// catalog and the mission browser data, both built from one `Datacore`
/// parse so the cold path pays the DCB cost once. Serialized whole into the
/// processed snapshot (`catalog.cook`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CookedData {
    pub blueprints: Vec<hearth_core::BpView>,
    pub missions: Vec<hearth_core::MissionView>,
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
    let cache_dir = cache::cache_dir_for(channel)?;

    // ── Tier 1: processed snapshot (sub-second) ───────────────────
    if let Some(cooked) = cache::try_load_processed(&cache_dir, &install) {
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
    if let Some((datacore, locale)) = cache::try_load_extract(&cache_dir, &install) {
        let cooked = cook::build_cooked(&datacore, &locale);
        cache::save_processed(&cache_dir, &install, &cooked);
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
    cache::save_extract(&cache_dir, &install, &assets);

    let locale_bytes =
        cache::read_locale_bytes(&assets).context("reading global.ini from Data.p4k")?;
    drop(assets);
    let locale = cache::build_locale_map(&locale_bytes)?;

    tracing::info!("building catalog + missions");
    let cooked = cook::build_cooked(&datacore, &locale);
    cache::save_processed(&cache_dir, &install, &cooked);

    tracing::info!(
        blueprints = cooked.blueprints.len(),
        missions = cooked.missions.len(),
        channel = ?channel,
        elapsed_ms = start.elapsed().as_millis(),
        "data built from live parse"
    );
    Ok(cooked)
}
