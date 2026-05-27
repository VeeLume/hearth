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
//! `LoadedScData::load_blocking` is sync + slow (~10s+ on first run,
//! Datacore parse is dominated by `record_store` codegen). Always call
//! via `tauri::async_runtime::spawn_blocking` so the Tauri runtime
//! thread stays responsive.

use anyhow::{Context, Result};
use hearth_core::BpView;
use sc_contracts::BlueprintPoolRegistry;
use sc_extract::{AssetConfig, AssetData, AssetSource, Datacore, DatacoreConfig, LocaleMap};

/// SC reference data + the LocaleMap needed to resolve display names.
/// Held on `AppState` behind a tokio Mutex so the heavy parse runs at
/// most once per app session.
///
/// **All heavy work happens inside `load_blocking`** — the BP catalog is
/// pre-built there so subsequent `blueprints()` calls are just slice
/// access. This matters because `BlueprintPoolRegistry::build` pulls in
/// sc-extract-generated's giant match statements, which overflow the
/// async worker's 2 MiB stack if run there. Blocking threads (where
/// `load_blocking` runs) get the OS default stack, large enough.
pub struct LoadedScData {
    /// Channel name (e.g. "LIVE") for the install we loaded from.
    pub channel: String,
    /// Pre-computed BP catalog. Built once at load time on the blocking
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
    /// Discover the first available SC install and load its reference
    /// data. Blocking — call via `spawn_blocking`.
    pub fn load_blocking() -> Result<Self> {
        let installs = sc_installs::discover().context("sc_installs::discover")?;
        let install = installs
            .into_iter()
            .next()
            .context("no Star Citizen installations detected")?;
        let channel = install.channel.to_string();
        let p4k_path = install.data_p4k();
        let assets = AssetSource::open(&p4k_path)
            .with_context(|| format!("opening Data.p4k for channel {channel}"))?;

        // Build LocaleMap from the english global.ini in the p4k.
        let ini_bytes = assets
            .read("Data/Localization/english/global.ini")
            .context("reading global.ini from p4k")?;
        let locale = build_locale_map(&ini_bytes)?;

        // Datacore parse — the slow step. `AssetConfig::minimal()` skips
        // the parse-time LocaleMap build. `DatacoreConfig::standard()`
        // builds LocalizedItemCache which BlueprintItem::display_name needs.
        tracing::info!("extracting AssetData");
        let asset_data = AssetData::extract(&assets, &AssetConfig::minimal())
            .context("AssetData::extract")?;
        tracing::info!("parsing Datacore");
        let datacore = Datacore::parse(&assets, &asset_data, &DatacoreConfig::standard())
            .context("Datacore::parse")?;
        drop(assets); // release file handles before doing anything else.

        // Pre-build the BP catalog on this (blocking) thread so the
        // command path never has to touch the registry on the async
        // worker (see struct docstring).
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

/// Parse global.ini bytes (UTF-16 LE with BOM) into a LocaleMap.
///
/// Locale-metadata suffixes (e.g. trailing `,P` on variant keys) are
/// stripped via `sc_extract::strip_locale_metadata` so the resulting
/// map keys match the DCB-reference form.
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
