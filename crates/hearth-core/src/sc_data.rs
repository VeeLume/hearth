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
    Blueprint, Cost, Duration as ScDuration, ItemCost, Recipe as ScRecipe, ResourceCost,
};

use crate::types::{BpView, Ingredient, IngredientKind, Recipe};

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
        category_raw: None,
        family_id: None,
        family_base_name: None,
        recipe: project_recipe(blueprint),
    }
}

/// Render a `Guid` as its hex-string form. This is the single point that
/// decides "how do GUIDs cross the IPC boundary," so callers stay consistent.
pub fn guid_string(guid: &Guid) -> String {
    format!("{guid}")
}

/// Flatten a `sc_crafting::Blueprint.tiers[0].recipe` into the lean
/// [`Recipe`] shape. Ingredient names are left `None` here — the loader
/// fills them from the `Resources` / `Items` indexes (which need a
/// `LocaleMap` to resolve `name_key`).
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
        collect_costs(cost, &mut ingredients);
    }
    Some(Recipe {
        craft_time_seconds,
        ingredients,
    })
}

fn duration_to_seconds(d: &ScDuration) -> f32 {
    (d.days as f32) * 86_400.0 + (d.hours as f32) * 3_600.0 + (d.minutes as f32) * 60.0 + d.seconds
}

/// Walk the polymorphic `Cost` tree and collect every leaf as an
/// `Ingredient`. SC 4.8 universally shapes mandatory costs as
/// `Select { N, [Select { 1, [<leaf>] }] }`, which this walk flattens to
/// a single list. Both leaf kinds are projected: `Resource` leaves
/// (ship-mined / refined materials, ~3.9k entries) and `Item` leaves
/// (discrete carried items — the hand-mined gems, ~294 entries).
/// Dormant `Other` variants have no live records and are skipped.
fn collect_costs(cost: &Cost, out: &mut Vec<Ingredient>) {
    match cost {
        Cost::Resource(rc) => {
            if let Some(ing) = ingredient_from_resource_cost(rc) {
                out.push(ing);
            }
        }
        Cost::Item(ic) => {
            if let Some(ing) = ingredient_from_item_cost(ic) {
                out.push(ing);
            }
        }
        Cost::Select { options, .. } => {
            for option in options {
                collect_costs(option, out);
            }
        }
        Cost::Other { .. } => {
            // Dormant variant fallback; nothing to project.
        }
    }
}

fn ingredient_from_resource_cost(rc: &ResourceCost) -> Option<Ingredient> {
    let guid = rc.resource?;
    let quantity_scu = rc.quantity.as_ref().and_then(|q| q.to_scu());
    Some(Ingredient {
        kind: IngredientKind::Resource,
        guid: guid_string(&guid),
        name: None,
        quantity_scu,
        count: None,
        min_quality: rc.min_quality,
    })
}

fn ingredient_from_item_cost(ic: &ItemCost) -> Option<Ingredient> {
    let guid = ic.entity_class?;
    Some(Ingredient {
        kind: IngredientKind::Item,
        guid: guid_string(&guid),
        name: None,
        quantity_scu: None,
        count: Some(ic.quantity),
        min_quality: ic.min_quality,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_holotable::resources::CargoQuantity;

    fn guid(b: u8) -> Guid {
        Guid::from_bytes([b; 16])
    }

    /// Regression: recipes mix `Resource` and `Item` cost leaves, and the
    /// `Item` leaves are the hand-mined gems (e.g. Hadanite ×13). Both
    /// must survive the cost-tree walk — the original code dropped every
    /// `Item` leaf on a stale "0 records in SC 4.8" assumption.
    #[test]
    fn collect_costs_projects_both_resource_and_item_leaves() {
        // SC 4.8 shape: Select { N, [Select{1,[Resource]}, Select{1,[Item]}] }.
        let tree = Cost::Select {
            count: 2,
            options: vec![
                Cost::Select {
                    count: 1,
                    options: vec![Cost::Resource(ResourceCost {
                        resource: Some(guid(1)),
                        quantity: Some(CargoQuantity::Centi(150)), // 1.5 SCU
                        min_quality: 0,
                    })],
                },
                Cost::Select {
                    count: 1,
                    options: vec![Cost::Item(ItemCost {
                        entity_class: Some(guid(2)),
                        quantity: 13,
                        min_quality: 1,
                    })],
                },
            ],
        };

        let mut out = Vec::new();
        collect_costs(&tree, &mut out);

        assert_eq!(out.len(), 2, "both leaves must be collected");

        let res = &out[0];
        assert_eq!(res.kind, IngredientKind::Resource);
        assert_eq!(res.guid, guid_string(&guid(1)));
        assert_eq!(res.quantity_scu, Some(1.5));
        assert_eq!(res.count, None);

        let item = &out[1];
        assert_eq!(item.kind, IngredientKind::Item);
        assert_eq!(item.guid, guid_string(&guid(2)));
        assert_eq!(item.count, Some(13));
        assert_eq!(item.quantity_scu, None);
        assert_eq!(item.min_quality, 1);
    }

    /// An item cost with no entity-class GUID has no identity to show, so
    /// it's dropped rather than surfaced as an unresolvable ingredient.
    #[test]
    fn item_cost_without_entity_class_is_skipped() {
        let tree = Cost::Item(ItemCost {
            entity_class: None,
            quantity: 1,
            min_quality: 0,
        });
        let mut out = Vec::new();
        collect_costs(&tree, &mut out);
        assert!(out.is_empty());
    }
}
