//! Loads SC reference data (blueprints, locale, …) from the local
//! game install.
//!
//! # Channel + platform selection
//!
//! `sc_installs::discover()` returns every installed channel sorted by
//! `Channel::priority()` (Live → Hotfix → PTU → EPTU → TechPreview).
//! We pick the first — the most-stable available install.
//!
//! The chosen install's `platform_id` (`'prod'` vs `'ptu'`) comes
//! straight from the launcher store (since sc-holotable v0.6) and
//! gets recorded on the loaded data so personal-state writes are
//! scoped correctly. If launcher-store fallback fired (log parsing),
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

use std::sync::mpsc;

use anyhow::{Context, Result};
use hearth_core::{BpView, Platform};
use sc_contracts::BlueprintPoolRegistry;
use sc_extract::{AssetConfig, AssetData, AssetSource, Datacore, DatacoreConfig, LocaleMap};
use sc_installs::Channel;

pub const LOADER_STACK_SIZE: usize = 32 * 1024 * 1024;

pub struct LoadedScData {
    /// Specific channel that produced this dataset (Live, Hotfix, …).
    pub channel: Channel,
    /// Stability grouping. `Prod` (Live + Hotfix) or `Ptu` (PTU + EPTU
    /// + TechPreview). Read from launcher store when available; falls
    /// back to a channel-based map otherwise.
    pub platform: Platform,
    /// RSI handle from the launcher store, if available. `None` if the
    /// launcher store couldn't be read or the identity block was empty
    /// (e.g. user has never signed into the launcher).
    pub handle: Option<String>,
    /// Pre-computed BP catalog.
    blueprints: Vec<BpView>,
    /// Held for future stage work (mission lookup, dynamic name
    /// resolution); not used during catalog reads.
    #[allow(dead_code)]
    datacore: Datacore,
    #[allow(dead_code)]
    locale: LocaleMap,
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
        let mut installs = sc_installs::discover().context("sc_installs::discover")?;
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
        let handle = match sc_installs::read_identity() {
            Ok(id) => Some(id.handle),
            Err(e) => {
                tracing::info!(
                    "launcher identity unavailable ({e}); handle stays unbound"
                );
                None
            }
        };

        let p4k_path = install.data_p4k();
        let assets = AssetSource::open(&p4k_path)
            .with_context(|| format!("opening Data.p4k for {channel:?}"))?;

        let ini_bytes = assets
            .read("Data/Localization/english/global.ini")
            .context("reading global.ini from p4k")?;
        let locale = build_locale_map(&ini_bytes)?;

        tracing::info!("extracting AssetData ({:?})", channel);
        let asset_data = AssetData::extract(&assets, &AssetConfig::minimal())
            .context("AssetData::extract")?;
        tracing::info!("parsing Datacore ({:?})", channel);
        let datacore = Datacore::parse(&assets, &asset_data, &DatacoreConfig::standard())
            .context("Datacore::parse")?;
        drop(assets);

        tracing::info!("building blueprint catalog");
        let blueprints = build_blueprints(&datacore, &locale);
        tracing::info!(
            count = blueprints.len(),
            channel = ?channel,
            platform = platform.as_str(),
            "blueprint catalog ready"
        );

        Ok(Self {
            channel,
            platform,
            handle,
            blueprints,
            datacore,
            locale,
        })
    }

    pub fn blueprints(&self) -> Vec<BpView> {
        self.blueprints.clone()
    }
}

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
