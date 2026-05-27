//! Single adapter point between Hearth and sc-holotable types.
//!
//! Concentrating contact with sc-holotable here keeps the blast radius
//! of upstream churn local — when sc-holotable bumps a breaking type,
//! only this module (plus its tests) needs to change.
//!
//! This module is *pure*. It contains conversions only; no file I/O,
//! no archive opening, no async. The actual SC data **loading** lives
//! in `hearth-app/src-tauri/src/sc_loader.rs` (it needs Tauri's
//! AppHandle / async runtime to be useful). Stage 2 wires the loader
//! to call the adapters here.

use sc_contracts::{BlueprintItem, BlueprintPool};

use crate::types::BpView;

/// Convert a sc-contracts `BlueprintItem` (with its containing pool) to
/// the lean `BpView` shape sent across the Tauri IPC boundary.
///
/// Display name resolution is intentionally not done here — it requires
/// a `LocaleMap` + `LocalizedItemCache` which are heavy and load-time
/// concerns. The loader will pass a name-resolver closure when Stage 2
/// arrives; for now `display_name` is `None`.
pub fn bp_view(item: &BlueprintItem, pool: &BlueprintPool) -> BpView {
    BpView {
        pool_guid: guid_string(&pool.guid),
        pool_name: pool.name.clone(),
        blueprint_record_guid: guid_string(&item.blueprint_record_guid),
        crafted_entity_guid: item.crafted_entity_guid.as_ref().map(guid_string),
        display_name: None,
        weight: item.weight,
    }
}

/// Render a sc-extract `Guid` as its hex-string form. This is the single
/// point that decides "how do GUIDs cross the IPC boundary," so callers
/// stay consistent.
pub fn guid_string(guid: &sc_extract::Guid) -> String {
    format!("{guid}")
}
