//! Loads SC reference data (blueprints, locale, …) from the local
//! game install.
//!
//! Pipeline (executed once per app session, cached on `AppState`):
//!
//! ```text
//! sc_installs::discover()
//!   → Installation (one of LIVE/PTU/Hotfix/EPTU/TechPreview)
//!   → AssetSource::open(install.data_p4k())
//!   → Data/Localization/english/global.ini  → UTF-16 LE decode → LocaleMap
//!   → AssetData::extract(AssetConfig::minimal())
//!   → Datacore::parse(DatacoreConfig::standard())  // also builds LocalizedItemCache
//!   → BlueprintPoolRegistry::build(&datacore)
//! ```
//!
//! # Stack-size workaround
//!
//! sc-extract-generated's `record_store` decoder has match arms deep
//! enough to overflow the default thread stack on Windows (~1 MiB for
//! secondary threads, ~2 MiB for tokio workers by default — neither is
//! enough). Pattern lifted from sc-langpatch's `preview::load_on_big_stack`
//! and bulkhead's `data::LOADER_STACK_SIZE`: spin up a dedicated
//! `std::thread` with an explicit 16 MiB stack, run the load to
//! completion, and `join()` the result back.

use std::sync::mpsc;

use anyhow::{Context, Result};
use hearth_core::BpView;
use sc_contracts::BlueprintPoolRegistry;
use sc_extract::{AssetConfig, AssetData, AssetSource, Datacore, DatacoreConfig, LocaleMap};

/// 32 MiB — matches bulkhead's `LOADER_STACK_SIZE`. 16 MiB *was*
/// enough to run the parse itself, but the mpsc receiver (which sits
/// on tokio's small-stack blocking pool) overflowed when the unboxed
/// `LoadedScData` struct moved through the channel. The fix is twofold:
/// box the result so only an 8-byte pointer crosses the bridge, and
/// give the loader thread plenty of headroom while we're at it.
pub const LOADER_STACK_SIZE: usize = 32 * 1024 * 1024;

/// SC reference data + the LocaleMap needed to resolve display names.
/// Held on `AppState` behind a tokio Mutex so the heavy parse runs at
/// most once per app session.
pub struct LoadedScData {
    /// Channel name (e.g. "LIVE") for the install we loaded from.
    pub channel: String,
    /// Pre-computed BP catalog. Built once at load time on the loader
    /// thread; returned by reference on every later call.
    blueprints: Vec<BpView>,
    /// Held for future stage work (mission lookup, name resolution of
    /// freshly-fetched records); not used during catalog access.
    #[allow(dead_code)]
    datacore: Datacore,
    #[allow(dead_code)]
    locale: LocaleMap,
}

impl LoadedScData {
    /// Spawn a dedicated big-stack thread, run the load on it, return
    /// a boxed result via mpsc. The result is boxed *inside* the loader
    /// thread so the move across the channel into a (small-stack)
    /// tokio blocking thread only copies an 8-byte pointer — moving
    /// the full struct through there overflows even with a generous
    /// stack on this side.
    pub async fn load_async() -> Result<Box<Self>> {
        let (tx, rx) = mpsc::channel::<Result<Box<Self>>>();
        std::thread::Builder::new()
            .name("hearth-sc-loader".into())
            .stack_size(LOADER_STACK_SIZE)
            .spawn(move || {
                let result = Self::load_inner().map(Box::new);
                let _ = tx.send(result);
            })
            .context("spawning sc-loader thread")?;
        tokio::task::spawn_blocking(move || rx.recv())
            .await
            .context("joining sc-loader bridge task")?
            .map_err(|_| anyhow::anyhow!("sc-loader sender dropped"))?
    }

    pub fn load_inner() -> Result<Self> {
        let installs = sc_installs::discover().context("sc_installs::discover")?;
        let install = installs
            .into_iter()
            .next()
            .context("no Star Citizen installations detected")?;
        let channel = install.channel.to_string();
        let p4k_path = install.data_p4k();
        let assets = AssetSource::open(&p4k_path)
            .with_context(|| format!("opening Data.p4k for channel {channel}"))?;

        let ini_bytes = assets
            .read("Data/Localization/english/global.ini")
            .context("reading global.ini from p4k")?;
        let locale = build_locale_map(&ini_bytes)?;

        tracing::info!("extracting AssetData");
        let asset_data = AssetData::extract(&assets, &AssetConfig::minimal())
            .context("AssetData::extract")?;
        tracing::info!("parsing Datacore");
        let datacore = Datacore::parse(&assets, &asset_data, &DatacoreConfig::standard())
            .context("Datacore::parse")?;
        drop(assets);

        tracing::info!("building blueprint catalog");
        let blueprints = build_blueprints(&datacore, &locale);
        tracing::info!(count = blueprints.len(), "blueprint catalog ready");

        Ok(Self {
            channel,
            blueprints,
            datacore,
            locale,
        })
    }

    /// Cheap slice-clone of the pre-built BP catalog.
    pub fn blueprints(&self) -> Vec<BpView> {
        self.blueprints.clone()
    }
}

fn build_blueprints(datacore: &Datacore, locale: &LocaleMap) -> Vec<BpView> {
    let registry = BlueprintPoolRegistry::build(datacore);
    let cache = &datacore.snapshot().localized_items;

    let mut out = Vec::new();
    for pool in registry.iter() {
        for item in &pool.items {
            let mut view = hearth_core::sc_data::bp_view(item, pool);
            view.display_name = item.display_name(cache, locale).map(|s| s.to_owned());
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
            let key = sc_extract::strip_locale_metadata(raw_key);
            let value = &line[eq_pos + 1..];
            map.set(key, value);
        }
    }
    Ok(map)
}
