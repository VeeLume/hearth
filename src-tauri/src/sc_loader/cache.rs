//! The layered snapshot cache that makes subsequent SC loads fast, plus the
//! `global.ini` locale decoding shared by the cold and warm paths.
//!
//! Two cache tiers, newest-wins, both keyed by SC `build_id` so a patch
//! invalidates them:
//!
//! 1. **Processed snapshot** (`catalog.cook`) — the cooked [`CookedData`]
//!    serialized whole. Sub-second; no parsing. Also invalidated by a
//!    [`HEARTH_CATALOG_COOK_VERSION`] bump (the cooked layout changed).
//! 2. **Raw extract snapshot** (`extract.snap`) — captured DCB + `global.ini`
//!    bytes; skips p4k extraction but still pays the DCB-parse cost.
//!
//! Snapshot failures (missing file, version mismatch, staleness, decode error)
//! are non-fatal: they log at info level and the orchestrator in
//! [`super::build_data`] falls through to the next tier. Cache files live under
//! `%APPDATA%/hearth[-dev]/cache/<channel>/`; atomic writes mean a crash
//! mid-save can't leave a half-written file behind.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use sc_holotable::asset::{
    AssetConfig, AssetData, AssetSource, Datacore, ExtractSnapshot, LocaleMap, ProcessedSnapshot,
    SnapshotCaptureConfig, snapshot_meta_from_install,
};
use sc_holotable::install::{Channel, Installation};

use super::CookedData;

/// Cook-format version for hearth's processed snapshot. Bump whenever the
/// cooked [`CookedData`] serde shape changes ([`hearth_core::BpView`] or
/// [`hearth_core::MissionView`] fields added/renamed/retyped) so older caches
/// invalidate cleanly via `Error::ProcessedSnapshotStale` instead of
/// deserializing into a silently-wrong shape. (18: CookedData gains
/// resource_names / location_names CRC→name maps and BpView ingredients gain
/// a `crc` match key, for the live resource inventory.)
const HEARTH_CATALOG_COOK_VERSION: u32 = 18;

pub(super) const EXTRACT_SNAPSHOT_NAME: &str = "extract.snap";
pub(super) const CATALOG_SNAPSHOT_NAME: &str = "catalog.cook";

/// Per-channel cache directory under Hearth's data root
/// (`%APPDATA%/hearth[-dev]/cache/<channel>/` on Windows). Honours the
/// dev/release namespace split via [`crate::app_data_root`].
pub(super) fn cache_dir_for(channel: Channel) -> Result<PathBuf> {
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
pub(super) fn try_load_processed(cache_dir: &Path, install: &Installation) -> Option<CookedData> {
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

pub(super) fn save_processed(cache_dir: &Path, install: &Installation, cooked: &CookedData) {
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
pub(super) fn try_load_extract(
    cache_dir: &Path,
    install: &Installation,
) -> Option<(Datacore, LocaleMap)> {
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

pub(super) fn save_extract(cache_dir: &Path, install: &Installation, assets: &AssetSource) {
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
pub(super) fn read_locale_bytes(source: &AssetSource) -> Result<Vec<u8>> {
    let (_, bytes) = source
        .find_and_read(|name| {
            let n = name.to_ascii_lowercase();
            n.ends_with("english\\global.ini") || n.ends_with("english/global.ini")
        })
        .context("searching for english/global.ini")?
        .ok_or_else(|| anyhow!("english/global.ini not present in archive/snapshot"))?;
    Ok(bytes)
}

pub(super) fn build_locale_map(bytes: &[u8]) -> Result<LocaleMap> {
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
