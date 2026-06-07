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

use sc_holotable::asset::{Guid, LocaleMap, class_crc};
use sc_holotable::crafting::{
    Blueprint, Cost, DisplayTransformation, Duration as ScDuration, GameplayProperties,
    GameplayPropertyModifier, ItemCost, Recipe as ScRecipe, ResourceCost, SlotName, ValueRange,
};

use crate::types::{
    BpView, CraftDetail, CraftModifier, Ingredient, IngredientKind, ModifierRange,
    ModifierTransform, Recipe, RecipeSlot,
};

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
        crc: Some(class_crc(&guid)),
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
        crc: Some(class_crc(&guid)),
        name: None,
        quantity_scu: None,
        count: Some(ic.quantity),
        min_quality: ic.min_quality,
    })
}

/// Project a blueprint's recipe into the rich per-slot [`CraftDetail`] used by
/// the crafting calculator. Unlike [`project_recipe`] (which flattens the cost
/// tree to a straight ingredient list for the catalog), this preserves the
/// **named material slots** and their **gameplay-property modifier curves**.
///
/// `gpps` resolves a modifier's property GUID to its display name / unit /
/// transform; `locale` resolves those keys to strings. `default_quality` is
/// the global `CraftingGlobalParams.default_composition_quality`, carried onto
/// the detail so a single fetch is self-contained. Ingredient *names* are left
/// `None` for the loader to fill (same as [`bp_view`]).
///
/// Returns `None` when the blueprint has no tier-0 recipe; a recipe with no
/// resolvable slots yields a `CraftDetail` with an empty `slots` vec.
pub fn craft_detail(
    blueprint: &Blueprint,
    gpps: &GameplayProperties,
    locale: &LocaleMap,
    default_quality: i32,
) -> Option<CraftDetail> {
    let recipe: &ScRecipe = blueprint.tiers.first()?.recipe.as_ref()?;
    let craft_time_seconds = recipe.craft_time.as_ref().map(duration_to_seconds);

    let mut raw = Vec::new();
    if let Some(costs) = &recipe.costs
        && let Some(mandatory) = &costs.mandatory
    {
        collect_slots(mandatory, &mut raw);
    }

    let slots = raw
        .into_iter()
        .map(|rs| RecipeSlot {
            slot_name: rs.name_info.and_then(|n| resolve_slot_name(n, locale)),
            ingredient: rs.ingredient,
            modifiers: rs
                .modifiers
                .iter()
                .map(|m| build_modifier(m, gpps, locale))
                .collect(),
        })
        .collect();

    Some(CraftDetail {
        blueprint_record_guid: guid_string(&blueprint.blueprint_record_guid),
        craft_time_seconds,
        default_quality,
        slots,
    })
}

/// A material slot discovered in the cost tree, before locale/property
/// resolution — borrows the slot label and modifiers from the live tree.
struct RawSlot<'a> {
    name_info: Option<&'a SlotName>,
    ingredient: Ingredient,
    modifiers: Vec<&'a GameplayPropertyModifier>,
}

/// Walk the mandatory cost tree and pull out each **material slot**. SC 4.8
/// shapes mandatory costs as `Select { N, [Select { 1, [<leaf>] }] }`: the
/// outer `Select` groups the slots, each inner `Select` is one named slot
/// ("Frame", "Cabling", …) holding the material leaf. A slot is any `Select`
/// that directly contains a `Resource`/`Item` leaf — its `name_info` is the
/// label and `gameplay_property_modifiers()` rolls up its (and the leaf's)
/// effect context. Container selects (options are all `Select`s) are
/// recursed through. A bare top-level leaf (no enclosing slot) surfaces as an
/// unnamed slot. First leaf wins when a slot offers material alternatives
/// (always one in SC 4.8).
fn collect_slots<'a>(cost: &'a Cost, out: &mut Vec<RawSlot<'a>>) {
    match cost {
        Cost::Select {
            name_info, options, ..
        } => {
            if let Some(ingredient) = options.iter().find_map(slot_ingredient) {
                out.push(RawSlot {
                    name_info: name_info.as_ref(),
                    ingredient,
                    modifiers: cost.gameplay_property_modifiers(),
                });
            } else {
                for option in options {
                    collect_slots(option, out);
                }
            }
        }
        Cost::Resource(_) | Cost::Item(_) => {
            if let Some(ingredient) = slot_ingredient(cost) {
                out.push(RawSlot {
                    name_info: None,
                    ingredient,
                    modifiers: cost.gameplay_property_modifiers(),
                });
            }
        }
        Cost::Other { .. } => {}
    }
}

/// The ingredient a `Resource`/`Item` leaf projects to, else `None`.
fn slot_ingredient(cost: &Cost) -> Option<Ingredient> {
    match cost {
        Cost::Resource(rc) => ingredient_from_resource_cost(rc),
        Cost::Item(ic) => ingredient_from_item_cost(ic),
        _ => None,
    }
}

/// Resolve a slot's display label, dropping empty / placeholder values
/// (`<= PLACEHOLDER =>` for not-yet-authored slots).
fn resolve_slot_name(name: &SlotName, locale: &LocaleMap) -> Option<String> {
    let resolved = locale.resolve(&name.display_name)?;
    if resolved.is_empty() || resolved.contains("PLACEHOLDER") {
        return None;
    }
    Some(resolved.to_owned())
}

/// Join a modifier to its property def for display metadata + project its
/// curve bands. Property name / unit resolve via `gpps` + `locale`; the
/// transform comes off the property def (`None` ⇒ raw ×factor display).
fn build_modifier(
    m: &GameplayPropertyModifier,
    gpps: &GameplayProperties,
    locale: &LocaleMap,
) -> CraftModifier {
    let prop = m.gameplay_property.and_then(|g| gpps.get(&g));
    let property_name = prop
        .and_then(|p| locale.resolve(&p.property_name_key))
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let unit_format = prop
        .and_then(|p| locale.resolve(&p.unit_format_key))
        .filter(|s| !s.is_empty() && !s.contains("EMPTY"))
        .map(str::to_owned);
    let transform = prop
        .and_then(|p| p.display_transformation.as_ref())
        .map(map_transform)
        .unwrap_or_else(modifier_transform_raw);
    let ranges = m.value_ranges.iter().filter_map(map_range).collect();
    CraftModifier {
        property_name,
        unit_format,
        transform,
        ranges,
    }
}

/// Flatten a `sc_crafting::DisplayTransformation` to the IPC-friendly tagged
/// struct. `Sequence` and dormant `Other` collapse to `raw` (show the raw
/// ×factor without a derived percent).
fn map_transform(t: &DisplayTransformation) -> ModifierTransform {
    match t {
        DisplayTransformation::Scale { factor } => ModifierTransform {
            kind: "scale".into(),
            scale_factor: Some(*factor),
        },
        DisplayTransformation::ConvertFactorToPercentChange => ModifierTransform {
            kind: "factor_to_percent".into(),
            scale_factor: None,
        },
        DisplayTransformation::ConvertFactorToNegatedPercentChange => ModifierTransform {
            kind: "factor_to_negated_percent".into(),
            scale_factor: None,
        },
        DisplayTransformation::ConvertValueToFactorOfBaseValue => ModifierTransform {
            kind: "value_to_factor".into(),
            scale_factor: None,
        },
        DisplayTransformation::Sequence(_) | DisplayTransformation::Other { .. } => {
            modifier_transform_raw()
        }
    }
}

fn modifier_transform_raw() -> ModifierTransform {
    ModifierTransform {
        kind: "raw".into(),
        scale_factor: None,
    }
}

/// Flatten a `sc_crafting::ValueRange` band. `Other` (dormant) bands are
/// dropped (the consumer can't evaluate them).
fn map_range(vr: &ValueRange) -> Option<ModifierRange> {
    match vr {
        ValueRange::Linear {
            start_quality,
            end_quality,
            modifier_at_start,
            modifier_at_end,
        } => Some(ModifierRange {
            additive: false,
            start_quality: *start_quality,
            end_quality: *end_quality,
            at_start: *modifier_at_start,
            at_end: *modifier_at_end,
        }),
        ValueRange::LinearIntegerAdditive {
            start_quality,
            end_quality,
            additive_at_start,
            additive_at_end,
        } => Some(ModifierRange {
            additive: true,
            start_quality: *start_quality,
            end_quality: *end_quality,
            at_start: *additive_at_start as f32,
            at_end: *additive_at_end as f32,
        }),
        ValueRange::Other { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_holotable::crafting::CostContext;
    use sc_holotable::resources::CargoQuantity;

    fn guid(b: u8) -> Guid {
        Guid::from_bytes([b; 16])
    }

    fn resource_cost(b: u8, centi: i32, min_quality: i32) -> ResourceCost {
        ResourceCost {
            resource: Some(guid(b)),
            quantity: Some(CargoQuantity::Centi(centi)),
            min_quality,
            context: vec![],
        }
    }

    /// Regression: recipes mix `Resource` and `Item` cost leaves, and the
    /// `Item` leaves are the hand-mined gems (e.g. Hadanite ×13). Both
    /// must survive the cost-tree walk — the original code dropped every
    /// `Item` leaf on a stale "0 records in SC 4.8" assumption.
    #[test]
    fn collect_costs_projects_both_resource_and_item_leaves() {
        // SC 4.8 shape: Select { N, [Select{1,[Resource]}, Select{1,[Item]}] }.
        let tree = Cost::Select {
            name_info: None,
            count: 2,
            context: vec![],
            options: vec![
                Cost::Select {
                    name_info: None,
                    count: 1,
                    context: vec![],
                    options: vec![Cost::Resource(resource_cost(1, 150, 0))], // 1.5 SCU
                },
                Cost::Select {
                    name_info: None,
                    count: 1,
                    context: vec![],
                    options: vec![Cost::Item(ItemCost {
                        entity_class: Some(guid(2)),
                        quantity: 13,
                        min_quality: 1,
                        context: vec![],
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
        // The CRC is the inventory-match key, hashed from the same GUID the
        // EntityGraph backend keys on.
        assert_eq!(res.crc, Some(class_crc(&guid(1))));
        assert_eq!(res.quantity_scu, Some(1.5));
        assert_eq!(res.count, None);

        let item = &out[1];
        assert_eq!(item.kind, IngredientKind::Item);
        assert_eq!(item.guid, guid_string(&guid(2)));
        assert_eq!(item.crc, Some(class_crc(&guid(2))));
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
            context: vec![],
        });
        let mut out = Vec::new();
        collect_costs(&tree, &mut out);
        assert!(out.is_empty());
    }

    /// The slot walk groups the per-material `Select`s and rolls up each
    /// slot's gameplay-property modifiers — the inner named `Select` is the
    /// slot, the outer one just groups them.
    #[test]
    fn collect_slots_groups_material_selects_with_modifiers() {
        let modifier = GameplayPropertyModifier {
            gameplay_property: None,
            value_ranges: vec![ValueRange::Linear {
                start_quality: 0,
                end_quality: 1000,
                modifier_at_start: 1.4,
                modifier_at_end: 0.6,
            }],
        };
        // Outer Select groups two slots; the first carries an effect at the
        // slot (inner Select) level, the second is plain.
        let tree = Cost::Select {
            name_info: None,
            count: 2,
            context: vec![],
            options: vec![
                Cost::Select {
                    name_info: None,
                    count: 1,
                    context: vec![CostContext::GameplayPropertyModifiers(vec![modifier])],
                    options: vec![Cost::Resource(resource_cost(1, 4, 0))],
                },
                Cost::Select {
                    name_info: None,
                    count: 1,
                    context: vec![],
                    options: vec![Cost::Resource(resource_cost(2, 2, 0))],
                },
            ],
        };

        let mut slots = Vec::new();
        collect_slots(&tree, &mut slots);

        assert_eq!(slots.len(), 2, "one slot per inner material Select");
        assert_eq!(slots[0].ingredient.guid, guid_string(&guid(1)));
        assert_eq!(slots[0].modifiers.len(), 1, "slot-level effect rolled up");
        assert_eq!(slots[1].ingredient.guid, guid_string(&guid(2)));
        assert!(slots[1].modifiers.is_empty());
    }

    #[test]
    fn map_range_projects_linear_and_additive_bands() {
        let mult = map_range(&ValueRange::Linear {
            start_quality: 0,
            end_quality: 1000,
            modifier_at_start: 1.4,
            modifier_at_end: 0.6,
        })
        .unwrap();
        assert!(!mult.additive);
        assert_eq!(mult.at_start, 1.4);
        assert_eq!(mult.at_end, 0.6);

        let add = map_range(&ValueRange::LinearIntegerAdditive {
            start_quality: 0,
            end_quality: 1000,
            additive_at_start: 10,
            additive_at_end: 50,
        })
        .unwrap();
        assert!(add.additive);
        assert_eq!(add.at_start, 10.0);
        assert_eq!(add.at_end, 50.0);

        assert!(
            map_range(&ValueRange::Other {
                type_name: "x".into(),
                struct_index: 0,
            })
            .is_none()
        );
    }

    #[test]
    fn map_transform_flattens_display_variants() {
        let scale = map_transform(&DisplayTransformation::Scale { factor: 1000.0 });
        assert_eq!(scale.kind, "scale");
        assert_eq!(scale.scale_factor, Some(1000.0));

        assert_eq!(
            map_transform(&DisplayTransformation::ConvertFactorToNegatedPercentChange).kind,
            "factor_to_negated_percent"
        );
        // Sequence collapses to raw (v1: show the ×factor only).
        assert_eq!(
            map_transform(&DisplayTransformation::Sequence(vec![])).kind,
            "raw"
        );
    }
}
