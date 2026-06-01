//! Stage 4 — owned-blueprints export for sc-langpatch.
//!
//! Hearth is the producer of the unidirectional JSON contract defined in
//! [`hearth_export`]. On every ownership change we rewrite
//! `%APPDATA%/hearth/exports/owned-blueprints.json` so sc-langpatch can read
//! the user's owned blueprints and render them differently.
//!
//! **Atomic write** — serialize to a `.tmp` sibling, then rename over the
//! target, so a consumer (or a crash mid-write) never sees a half-written
//! file. `std::fs::rename` replaces the destination on both Windows and Unix.
//!
//! The caller treats this as **best-effort**: a write failure is logged, not
//! surfaced, so a transient FS error can't break an ownership toggle — the
//! file is regenerated on the next change anyway.

use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::{Context, Result};
use hearth_export::{EXPORT_RELATIVE_PATH, OwnedBlueprints};
use sc_holotable::asset::Guid;

/// Absolute export path under the platform data dir
/// (`%APPDATA%/hearth/exports/owned-blueprints.json` on Windows). `None`
/// when the OS data dir can't be resolved.
pub fn export_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join(EXPORT_RELATIVE_PATH))
}

/// Build the [`OwnedBlueprints`] contract from the active scope's owned guid
/// strings and write it atomically. Unparseable guids are skipped (they'd
/// have to be corrupt — stored guids come from sc-holotable's own hex form).
pub fn write_owned(owned_guids: &[String]) -> Result<()> {
    let path = export_path().context("no platform data dir for owned-blueprints export")?;

    let owned: HashSet<Guid> = owned_guids
        .iter()
        .filter_map(|s| match s.parse::<Guid>() {
            Ok(g) => Some(g),
            Err(e) => {
                tracing::warn!("skipping unparseable owned guid {s:?} in export: {e}");
                None
            }
        })
        .collect();
    let doc = OwnedBlueprints { owned };
    let json = serde_json::to_vec_pretty(&doc).context("serializing owned-blueprints")?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating export dir {}", parent.display()))?;
    }

    // Atomic replace: write a temp sibling, then rename over the target.
    let mut tmp = path.clone();
    tmp.as_mut_os_string().push(".tmp");
    std::fs::write(&tmp, &json).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("renaming {} -> {}", tmp.display(), path.display()))?;

    tracing::debug!(
        count = doc.owned.len(),
        path = %path.display(),
        "wrote owned-blueprints export"
    );
    Ok(())
}
