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
use sc_holotable::crafting::{
    Blueprint, Cost, Duration as ScDuration, Recipe as ScRecipe, ResourceCost,
};

use crate::types::{BpView, Ingredient, Recipe};

/// Convert a `sc_crafting::Blueprint` to the lean `BpView` shape sent
/// across the Tauri IPC boundary.
///
/// Display name + item classification + resource-name resolution are
/// intentionally not done here — they require a `LocaleMap` / `Items` /
/// `Resources` index which are heavy load-time concerns. The loader
/// fills those fields after construction. Recipe **structure** (the
/// flat ingredient list + craft time) IS done here because it's a pure
/// projection from the blueprint, with no locale dependency; resource
/// names get filled separately by the loader.
pub fn bp_view(blueprint: &Blueprint) -> BpView {
    BpView {
        blueprint_record_guid: guid_string(&blueprint.blueprint_record_guid),
        crafted_entity_guid: blueprint.crafted_entity_guid().as_ref().map(guid_string),
        display_name: None,
        item_type: None,
        item_sub_type: None,
        recipe: project_recipe(blueprint),
    }
}

/// Render a `Guid` as its hex-string form. This is the single point that
/// decides "how do GUIDs cross the IPC boundary," so callers stay consistent.
pub fn guid_string(guid: &Guid) -> String {
    format!("{guid}")
}

/// Flatten a `sc_crafting::Blueprint.tiers[0].recipe` into the lean
/// [`Recipe`] shape. Resource names are left `None` here — the loader
/// fills them with the `Resources` index (which needs a `LocaleMap` to
/// resolve `name_key`).
///
/// Returns `None` if the blueprint has no tier-0 recipe at all
/// (extremely rare); a recipe with zero ingredients is still returned
/// (e.g. an instant-craft hypothetical with only a time cost).
fn project_recipe(blueprint: &Blueprint) -> Option<Recipe> {
    let recipe: &ScRecipe = blueprint.tiers.first()?.recipe.as_ref()?;
    let craft_time_seconds = recipe.craft_time.as_ref().map(duration_to_seconds);
    let mut ingredients = Vec::new();
    if let Some(costs) = &recipe.costs
        && let Some(cost) = &costs.mandatory
    {
        collect_resource_costs(cost, &mut ingredients);
    }
    Some(Recipe { craft_time_seconds, ingredients })
}

fn duration_to_seconds(d: &ScDuration) -> f32 {
    (d.days as f32) * 86_400.0
        + (d.hours as f32) * 3_600.0
        + (d.minutes as f32) * 60.0
        + d.seconds
}

/// Walk the polymorphic `Cost` tree and collect every `Resource(...)`
/// leaf as an `Ingredient`. SC 4.8 universally shapes mandatory costs
/// as `Select { N, [Select { 1, [Resource(rc)] }] }`, which this walk
/// flattens to a single list. Item costs and dormant `Other` variants
/// are silently skipped (zero records in live data); add typed handling
/// when CIG starts populating them.
fn collect_resource_costs(cost: &Cost, out: &mut Vec<Ingredient>) {
    match cost {
        Cost::Resource(rc) => {
            if let Some(ing) = ingredient_from_resource_cost(rc) {
                out.push(ing);
            }
        }
        Cost::Select { options, .. } => {
            for option in options {
                collect_resource_costs(option, out);
            }
        }
        Cost::Item(_) | Cost::Other { .. } => {
            // Item costs: 0 records in SC 4.8 — wire when populated.
            // Other: dormant variant fallback; nothing to project.
        }
    }
}

fn ingredient_from_resource_cost(rc: &ResourceCost) -> Option<Ingredient> {
    let guid = rc.resource?;
    let quantity_scu = rc.quantity.as_ref().and_then(|q| q.to_scu());
    Some(Ingredient {
        resource_guid: guid_string(&guid),
        resource_name: None,
        quantity_scu,
        min_quality: rc.min_quality,
    })
}
