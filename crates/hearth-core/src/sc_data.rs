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

use sc_holotable::asset::Guid;
use sc_holotable::crafting::Blueprint;

use crate::types::BpView;

/// Convert a `sc_crafting::Blueprint` to the lean `BpView` shape sent
/// across the Tauri IPC boundary.
///
/// Display name + item classification are intentionally not resolved here —
/// they require a `LocaleMap` / `Items` index which are heavy load-time
/// concerns. The loader fills `display_name` / `item_type` / `item_sub_type`
/// after construction; keeping this adapter pure (conversions only) bounds
/// the sc-holotable blast radius to this module.
pub fn bp_view(blueprint: &Blueprint) -> BpView {
    BpView {
        blueprint_record_guid: guid_string(&blueprint.blueprint_record_guid),
        crafted_entity_guid: blueprint.crafted_entity_guid().as_ref().map(guid_string),
        display_name: None,
        item_type: None,
        item_sub_type: None,
    }
}

/// Render a `Guid` as its hex-string form. This is the single point that
/// decides "how do GUIDs cross the IPC boundary," so callers stay consistent.
pub fn guid_string(guid: &Guid) -> String {
    format!("{guid}")
}
