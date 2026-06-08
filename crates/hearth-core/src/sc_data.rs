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

use std::collections::{HashMap, HashSet};

use sc_holotable::armor::{Armor, ArmorStats, DamageResistance, ResistanceEntry};
use sc_holotable::asset::{Guid, LocaleMap, class_crc};
use sc_holotable::crafting::{
    Blueprint, Cost, DisplayTransformation, Duration as ScDuration, GameplayProperties,
    GameplayPropertyModifier, GameplayStat, ItemCost, Recipe as ScRecipe, ResourceCost, SlotName,
    ValueRange,
};
use sc_holotable::fps_weapons::{Damage as FpsDamage, FpsWeaponStats, FpsWeapons};
use sc_holotable::ship_components::{ShipComponentStats, ShipComponents};
use sc_holotable::ship_weapons::{Damage as ShipDamage, ShipWeaponStats, ShipWeapons};

use crate::types::{
    BpView, CraftDetail, CraftModifier, Ingredient, IngredientKind, ModifierRange,
    ModifierTransform, ProductStat, Recipe, RecipeSlot,
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
/// **named material slots** and their **gameplay-property modifier curves**,
/// plus the crafted item's **product-stat sheet** (its full base stats, with the
/// recipe-reshaped ones linked to their modifiers).
///
/// `gpps` resolves a modifier's property GUID to its display name / unit /
/// transform / typed stat; `sheets` supplies the crafted item's full base-stat
/// sheet (the per-domain `FpsWeapons` / `Armor` / `ShipComponents` /
/// `ShipWeapons` indexes); `locale` resolves the display keys to strings.
/// `default_quality` is the global `CraftingGlobalParams.default_composition_quality`,
/// carried onto the detail so a single fetch is self-contained. Ingredient
/// *names* are left `None` for the loader to fill (same as [`bp_view`]).
///
/// Returns `None` when the blueprint has no tier-0 recipe; a recipe with no
/// resolvable slots yields a `CraftDetail` with an empty `slots` vec.
pub fn craft_detail(
    blueprint: &Blueprint,
    gpps: &GameplayProperties,
    sheets: &BaseStatSheets,
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

    let entity_guid = blueprint.crafted_entity_guid();
    let product_stats = build_product_stats(&raw, gpps, sheets, locale, entity_guid.as_ref());

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
        product_stats,
    })
}

/// Build the crafted item's product-stat sheet: its **full base stats** overlaid
/// with the recipe's gameplay-property modifiers. Base rows come from the
/// crafted entity's domain sheet ([`BaseStatSheets::rows_for`]); each row that
/// matches a recipe modifier (by typed [`GameplayStat`]) carries that modifier's
/// `gpp_guid` + transform so the UI aggregates it live against the quality
/// sliders. Recipe modifiers with no base row (tractor / hull-scraping — no
/// static absolute) are appended as percent-only rows. Returns empty when the
/// recipe touches no stats and the item has no base sheet.
fn build_product_stats(
    raw: &[RawSlot],
    gpps: &GameplayProperties,
    sheets: &BaseStatSheets,
    locale: &LocaleMap,
    entity: Option<&Guid>,
) -> Vec<ProductStat> {
    // The recipe's distinct gameplay properties (first-seen order), and the
    // typed stat each maps to — the overlay key onto the base rows. Keyed by the
    // stat's debug string (GameplayStat isn't Hash upstream); stable both sides.
    let mut seen = HashSet::new();
    let mut recipe_gpps: Vec<Guid> = Vec::new();
    let mut stat_to_gpp: HashMap<String, Guid> = HashMap::new();
    for rs in raw {
        for m in &rs.modifiers {
            if m.value_ranges.is_empty() {
                continue;
            }
            if let Some(g) = m.gameplay_property
                && seen.insert(g)
            {
                recipe_gpps.push(g);
                if let Some(p) = gpps.get(&g) {
                    stat_to_gpp.entry(stat_key(&p.stat())).or_insert(g);
                }
            }
        }
    }

    // Base-stat rows from the crafted item's domain sheet, each overlaid with
    // the recipe modifier that drives it (if any).
    let base_rows = entity.map(|e| sheets.rows_for(e)).unwrap_or_default();
    let mut out: Vec<ProductStat> = Vec::with_capacity(base_rows.len() + recipe_gpps.len());
    let mut covered: HashSet<String> = HashSet::new();
    for r in base_rows {
        let key = r.stat.as_ref().map(stat_key);
        if let Some(k) = &key {
            covered.insert(k.clone());
        }
        let gpp = key.as_ref().and_then(|k| stat_to_gpp.get(k)).copied();
        out.push(ProductStat {
            group: r.group.map(str::to_owned),
            label: r.label,
            gpp_guid: gpp.map(|g| guid_string(&g)),
            unit: r.unit.to_owned(),
            higher_is_better: r.stat.as_ref().and_then(higher_is_better),
            base: Some(r.base),
        });
    }

    // Recipe properties with no base row (no static absolute) → percent-only.
    for g in recipe_gpps {
        let stat = gpps.get(&g).map(|p| p.stat());
        if let Some(s) = &stat
            && covered.contains(&stat_key(s))
        {
            continue;
        }
        let label = gpps
            .get(&g)
            .and_then(|p| locale.resolve(&p.property_name_key))
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| "Effect".to_owned());
        out.push(ProductStat {
            group: None,
            label,
            gpp_guid: Some(guid_string(&g)),
            unit: String::new(),
            higher_is_better: stat.as_ref().and_then(higher_is_better),
            base: None,
        });
    }

    out
}

/// Whether a *higher* value is better for a [`GameplayStat`] — the buff/nerf
/// sense the UI colours by. Recoil / spread / quantum-fuel / cold-protection are
/// lower-is-better (a decrease is a buff); the rest are higher-is-better.
/// `None` for unmodelled stats (the UI falls back to raw sign).
fn higher_is_better(stat: &GameplayStat) -> Option<bool> {
    use GameplayStat::*;
    Some(match stat {
        WeaponFireRate | WeaponDamage | ArmorDamageMitigation | ArmorTemperatureMax
        | ArmorRadiationDissipation | Integrity | QuantumSpeed | ShieldMaxHealth
        | CoolantGeneration | PowerGeneration | RadarMinAimAssist | RadarMaxAimAssist => true,
        WeaponRecoilKick | WeaponRecoilHandling | WeaponRecoilSmoothness | WeaponSpread
        | QuantumFuelRequirement | ArmorTemperatureMin => false,
        Unknown(_) => return None,
    })
}

/// Stable string key for a [`GameplayStat`] (it isn't `Hash`/`Eq`-keyable
/// upstream). The debug form is stable across both match sides.
fn stat_key(stat: &GameplayStat) -> String {
    format!("{stat:?}")
}

/// The per-domain base-stat sheets, bundled so [`craft_detail`] can resolve a
/// crafted item's full base stats regardless of its domain (a crafted entity is
/// covered by exactly one sheet).
pub struct BaseStatSheets<'a> {
    pub fps_weapons: &'a FpsWeapons,
    pub armor: &'a Armor,
    pub ship_components: &'a ShipComponents,
    pub ship_weapons: &'a ShipWeapons,
}

impl BaseStatSheets<'_> {
    /// Flatten the crafted entity's base-stat sheet into display rows (empty if
    /// no domain sheet covers it).
    fn rows_for(&self, entity: &Guid) -> Vec<BaseStatRow> {
        if let Some(s) = self.fps_weapons.get(entity) {
            fps_weapon_rows(s)
        } else if let Some(s) = self.armor.get(entity) {
            armor_rows(s)
        } else if let Some(s) = self.ship_components.get(entity) {
            ship_component_rows(s)
        } else if let Some(s) = self.ship_weapons.get(entity) {
            ship_weapon_rows(s)
        } else {
            Vec::new()
        }
    }
}

/// One base-stat row before recipe-modifier overlay: display label + unit + base
/// value (display units), and the typed stat it maps to (for matching a recipe
/// modifier). `stat = None` for stats no recipe modifies (always shown as-is).
struct BaseStatRow {
    group: Option<&'static str>,
    label: String,
    unit: &'static str,
    stat: Option<GameplayStat>,
    base: f32,
}

fn row(
    group: Option<&'static str>,
    label: impl Into<String>,
    unit: &'static str,
    stat: Option<GameplayStat>,
    base: f32,
) -> BaseStatRow {
    BaseStatRow {
        group,
        label: label.into(),
        unit,
        stat,
        base,
    }
}

/// Largest non-zero damage component → its type name; the value is the total
/// across all types (mirrors `GameplayStat::WeaponDamage` = `Damage::total`).
fn dominant_damage(
    phys: f32,
    energy: f32,
    distortion: f32,
    thermal: f32,
    bio: f32,
    stun: f32,
    total: f32,
) -> (&'static str, f32) {
    let kinds = [
        ("Physical", phys),
        ("Energy", energy),
        ("Distortion", distortion),
        ("Thermal", thermal),
        ("Biochemical", bio),
        ("Stun", stun),
    ];
    let dom = kinds
        .into_iter()
        .fold(("", 0.0_f32), |best, cur| if cur.1 > best.1 { cur } else { best });
    (dom.0, total)
}

fn damage_label(ty: &str, per: &str) -> String {
    if ty.is_empty() {
        format!("Damage / {per}")
    } else {
        format!("{ty} Damage / {per}")
    }
}

fn fps_weapon_rows(s: &FpsWeaponStats) -> Vec<BaseStatRow> {
    let mut out = Vec::new();
    if let Some(fr) = s.fire_rate {
        out.push(row(None, "Fire Rate", " RPM", Some(GameplayStat::WeaponFireRate), fr as f32));
    }
    if let Some(d) = &s.damage {
        let (ty, val) = fps_dominant(d);
        out.push(row(None, damage_label(ty, "Shot"), "", Some(GameplayStat::WeaponDamage), val));
    }
    if let Some(v) = s.recoil_pitch {
        out.push(row(None, "Recoil Pitch", "°", Some(GameplayStat::WeaponRecoilKick), v));
    }
    if let Some(v) = s.recoil_yaw {
        out.push(row(None, "Recoil Yaw", "°", Some(GameplayStat::WeaponRecoilHandling), v));
    }
    if let Some(v) = s.recoil_smooth {
        out.push(row(None, "Recoil Smoothing", " s", Some(GameplayStat::WeaponRecoilSmoothness), v));
    }
    if let Some(v) = s.spread_max {
        out.push(row(None, "Spread", "°", Some(GameplayStat::WeaponSpread), v));
    }
    if let Some(v) = s.ammo_speed {
        out.push(row(None, "Ammo Speed", " m/s", None, v));
    }
    if let Some(v) = s.mag_size {
        out.push(row(None, "Magazine", "", None, v as f32));
    }
    out
}

fn fps_dominant(d: &FpsDamage) -> (&'static str, f32) {
    dominant_damage(
        d.physical,
        d.energy,
        d.distortion,
        d.thermal,
        d.biochemical,
        d.stun,
        d.total(),
    )
}

fn ship_dominant(d: &ShipDamage) -> (&'static str, f32) {
    dominant_damage(
        d.physical,
        d.energy,
        d.distortion,
        d.thermal,
        d.biochemical,
        d.stun,
        d.total(),
    )
}

fn armor_rows(s: &ArmorStats) -> Vec<BaseStatRow> {
    let mut out = Vec::new();
    if let Some(dr) = &s.damage_resistance {
        for (name, entry) in resistance_entries(dr) {
            if let Some(e) = entry {
                // Mitigation fraction (1 − damage-taken multiplier), shown as %.
                let mitigation = (1.0 - e.multiplier) * 100.0;
                out.push(row(
                    Some("Damage Resistance"),
                    name,
                    "%",
                    Some(GameplayStat::ArmorDamageMitigation),
                    mitigation,
                ));
            }
        }
    }
    if let Some(v) = s.temp_resistance_min {
        out.push(row(Some("Temperature"), "Min", "°C", Some(GameplayStat::ArmorTemperatureMin), v));
    }
    if let Some(v) = s.temp_resistance_max {
        out.push(row(Some("Temperature"), "Max", "°C", Some(GameplayStat::ArmorTemperatureMax), v));
    }
    if let Some(v) = s.radiation_dissipation {
        out.push(row(
            Some("Radiation"),
            "Dissipation",
            " REM/s",
            Some(GameplayStat::ArmorRadiationDissipation),
            v,
        ));
    }
    if let Some(v) = s.radiation_capacity {
        out.push(row(Some("Radiation"), "Capacity", " REM", None, v));
    }
    out
}

fn resistance_entries(
    dr: &DamageResistance,
) -> [(&'static str, &Option<ResistanceEntry>); 6] {
    [
        ("Physical", &dr.physical),
        ("Energy", &dr.energy),
        ("Distortion", &dr.distortion),
        ("Thermal", &dr.thermal),
        ("Biochemical", &dr.biochemical),
        ("Stun", &dr.stun),
    ]
}

fn ship_component_rows(s: &ShipComponentStats) -> Vec<BaseStatRow> {
    let mut out = Vec::new();
    if let Some(v) = s.integrity_hp {
        out.push(row(None, "Integrity", " HP", Some(GameplayStat::Integrity), v));
    }
    if let Some(v) = s.quantum_drive_speed {
        out.push(row(None, "Quantum Speed", " Mm/s", Some(GameplayStat::QuantumSpeed), v));
    }
    if let Some(v) = s.quantum_fuel_requirement {
        out.push(row(
            None,
            "Quantum Fuel / Distance",
            "",
            Some(GameplayStat::QuantumFuelRequirement),
            v,
        ));
    }
    if let Some(v) = s.shield_max_health {
        out.push(row(None, "Shield Capacity", " HP", Some(GameplayStat::ShieldMaxHealth), v));
    }
    if let Some(v) = s.shield_regen {
        out.push(row(None, "Shield Regen", " HP/s", None, v));
    }
    if let Some(v) = s.coolant_rate {
        out.push(row(None, "Coolant Output", "/s", Some(GameplayStat::CoolantGeneration), v));
    }
    if let Some(v) = s.power_output {
        out.push(row(None, "Power Output", " pips", Some(GameplayStat::PowerGeneration), v));
    }
    if let Some(v) = s.radar_aim_assist_min {
        out.push(row(None, "Aim-Assist (Min)", " m", Some(GameplayStat::RadarMinAimAssist), v));
    }
    if let Some(v) = s.radar_aim_assist_max {
        out.push(row(None, "Aim-Assist (Max)", " m", Some(GameplayStat::RadarMaxAimAssist), v));
    }
    out
}

fn ship_weapon_rows(s: &ShipWeaponStats) -> Vec<BaseStatRow> {
    let mut out = Vec::new();
    if let Some(v) = s.integrity_hp {
        out.push(row(None, "Integrity", " HP", Some(GameplayStat::Integrity), v));
    }
    if let Some(d) = &s.damage {
        let (ty, val) = ship_dominant(d);
        let per = if s.is_beam { "s" } else { "Shot" };
        out.push(row(None, damage_label(ty, per), "", Some(GameplayStat::WeaponDamage), val));
    }
    if let Some(fr) = s.fire_rate {
        out.push(row(None, "Fire Rate", " RPM", Some(GameplayStat::WeaponFireRate), fr as f32));
    }
    if let Some(v) = s.ammo_speed {
        out.push(row(None, "Ammo Speed", " m/s", None, v));
    }
    out
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
        gpp_guid: m.gameplay_property.as_ref().map(guid_string),
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
