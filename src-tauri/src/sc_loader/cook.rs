//! Cooking a parsed `Datacore` into Hearth's reference data — the blueprint
//! catalog and the mission browser list. Both products are built from one
//! `Datacore` parse (the cold path pays the DCB cost once) and serialized
//! whole into the processed snapshot via [`super::CookedData`].
//!
//! Each builder makes its own indices over the same datacore (cheap relative
//! to the parse), so they stay self-contained.

use std::collections::{HashMap, HashSet};

use hearth_core::sc_data::guid_string;
use hearth_core::{
    BpPoolReward, BpRewardEntry, BpView, ItemRewardView, MissionView, RepRewardView,
    ScripRewardView,
};
use sc_holotable::asset::{Datacore, LocaleKey, LocaleMap, RecordPaths, class_crc};
use sc_holotable::crafting::{Blueprints, Categories, Process};
use sc_holotable::items::{ItemCatalog, Items};
use sc_holotable::locations::Locations;
use sc_holotable::missions::{Mission, Missions, RewardAmount, RewardCurrencies};
use sc_holotable::resources::Resources;

use super::CookedData;

/// Cook all products from one parsed `Datacore`. Each builder makes its own
/// indices (cheap relative to the DCB parse this shares), so they stay
/// self-contained.
pub(super) fn build_cooked(datacore: &Datacore, locale: &LocaleMap) -> CookedData {
    CookedData {
        blueprints: build_blueprints(datacore, locale),
        missions: build_missions(datacore, locale),
        resource_names: build_resource_names(datacore, locale),
        location_names: build_location_names(datacore, locale),
    }
}

/// `class_crc(ResourceType.guid) → display name` for every resource. The
/// inventory sync keys sc-dossier's wire `resource_id` straight into this map,
/// so material names resolve without carrying a `LocaleMap` past the cook.
fn build_resource_names(datacore: &Datacore, locale: &LocaleMap) -> HashMap<u32, String> {
    let resources = Resources::build(datacore.records());
    let mut out = HashMap::new();
    for r in resources.all() {
        if let Some(name) = locale.resolve(&r.name_key)
            && !name.is_empty()
        {
            out.insert(class_crc(&r.guid), name.to_owned());
        }
    }
    out
}

/// `class_crc(StarMapObject.guid) → place name` for every universe location.
/// Resolves an inventory stack's `Location` / `Hangar` place CRC to a name.
fn build_location_names(datacore: &Datacore, locale: &LocaleMap) -> HashMap<u32, String> {
    let locations = Locations::build(datacore.records());
    let mut out = HashMap::new();
    for (guid, loc) in locations.iter() {
        if let Some(name) = loc.display_name(locale)
            && !name.is_empty()
        {
            out.insert(class_crc(guid), name.to_owned());
        }
    }
    out
}

fn build_blueprints(datacore: &Datacore, locale: &LocaleMap) -> Vec<BpView> {
    // Index passes over the same datacore:
    //   - Items        — entity name keys + typed Type/SubType
    //   - Blueprints   — the full crafting catalog (Blueprint + tier-0 Recipe)
    //   - Resources    — name_key + density for each ResourceType
    //   - RecordPaths  — Categories::build needs the path/name lookup
    //                    (categories are empty marker records — identity
    //                    is the record's name, exposed via RecordPaths)
    //   - Categories   — CIG-authored crafting taxonomy (20 entries in
    //                    SC 4.8: FPSWeapons, FPSArmours, Medical,
    //                    VehicleWeaponsS1-6, ...)
    //   - ItemCatalog  — variant grouping (paint / skin / "Modified"
    //                    variants of one model share a model id)
    // Items is the only index that's also passed downstream (Blueprints
    // and ItemCatalog both need it).
    let items = Items::build(datacore.records());
    let catalog = Blueprints::build(datacore, &items);
    let resources = Resources::build(datacore.records());
    let paths = RecordPaths::build(datacore);
    let categories = Categories::build(&paths);
    // ItemCatalog clusters gear into Models (one design + one slot: an
    // item and its colorway variants) grouped under Collections (the
    // models that read as the same design across slots/accessories, e.g.
    // "Geist Armor"). Grouping is display-name driven (Items + &LocaleMap);
    // &RecordPaths is used to pick the canonical base member of each model.
    let item_catalog = ItemCatalog::build(&items, &paths, locale);

    let mut out = Vec::new();
    for blueprint in catalog.iter() {
        // Filter to Creation blueprints — the ones that craft a real
        // entity. Dormant non-Creation processes (refining/repair/etc
        // — all 0 records in SC 4.8 but the schema reserves them)
        // would surface here with no crafted_entity_guid; skip them.
        if !matches!(blueprint.process, Process::Creation { .. }) {
            continue;
        }
        let mut view = hearth_core::sc_data::bp_view(blueprint);
        view.display_name = blueprint.display_name(locale).map(|s| s.to_owned());
        if let Some(entity_guid) = blueprint.crafted_entity_guid() {
            // item_type/item_sub_type are typed enums; the IPC boundary
            // carries their DCB-string form (what itemTypes.ts keys on).
            view.item_type = items
                .item_type(&entity_guid)
                .map(|t| t.as_dcb_str().to_owned());
            view.item_sub_type = items
                .item_sub_type(&entity_guid)
                .map(|t| t.as_dcb_str().to_owned());
        }
        // sc-crafting category — CIG-authored grouping. Strip the
        // verbose record-class prefix so the IPC carries just the
        // semantic name ("FPSArmours", "VehicleWeaponsS3", ...).
        if let Some(cat_guid) = blueprint.category
            && let Some(cat) = categories.get(&cat_guid)
        {
            view.category_raw = Some(
                cat.name
                    .strip_prefix("BlueprintCategoryRecord.")
                    .unwrap_or(&cat.name)
                    .to_owned(),
            );
        }
        // Variant-bundling key — ItemCatalog returns Some for any gear
        // item it grouped into a model (the solo fallback covers items
        // with no other signal), so this falls back to the raw guid when
        // the entity isn't gear (ship components, props) or the BP has
        // no crafted entity at all.
        if let Some(entity_guid) = blueprint.crafted_entity_guid() {
            view.family_id = Some(
                item_catalog
                    .model_id_of(&entity_guid)
                    .map(str::to_owned)
                    .unwrap_or_else(|| entity_guid.to_string()),
            );

            // Model base name — resolved through Items + LocaleMap so
            // the catalog UI's bundle row header reads the canonical
            // base item's name even when only variants are blueprinted.
            // All members of one model resolve to the same base name
            // here; the small redundant work per-BP is cheaper than
            // restructuring the loop to pre-compute per-model.
            if let Some(base_guid) = item_catalog.base_of(&entity_guid)
                && let Some(name_key) = items.name_key(&base_guid)
                && let Some(name) = locale.resolve(name_key)
                && !name.is_empty()
            {
                view.family_base_name = Some(name.to_owned());
            }
        }
        // Resolve each ingredient's name (bp_view leaves it None because
        // it has no Resources/Items/LocaleMap access). Resource costs
        // resolve via Resources, item costs (hand-mined gems) via Items.
        if let Some(recipe) = view.recipe.as_mut() {
            fill_ingredient_names(&mut recipe.ingredients, &resources, &items, locale);
        }
        out.push(view);
    }
    out
}

/// Build the mission browser data. CIG spawns one contract per offered
/// locality, so the raw list is thousands of near-duplicates; we **pool**
/// contracts that share a `(title_key, description_key)` into one template
/// (the mission a player perceives) and aggregate their localities into
/// `regions`. Per template we surface the reward axes (aUEC, scrip,
/// reputation, item unlocks, blueprint pools — the last resolved through
/// `Missions::blueprints`), a one-line encounter banner, and the count of
/// pooled instances.
fn build_missions(datacore: &Datacore, locale: &LocaleMap) -> Vec<MissionView> {
    let items = Items::build(datacore.records());
    let currencies = RewardCurrencies::build(datacore);
    let missions = Missions::build(datacore);
    let pools = &missions.blueprints;
    let localities = &missions.localities;

    // Group contracts into templates. Localities are *aggregated* (same
    // mission offered in many places = one row with a region list), not split
    // — splitting by location would re-introduce the duplication we set out to
    // remove. Templates are kept distinct by the axes that make missions
    // genuinely different:
    //   - reward identity (faction-specific pools, rep faction, scrip, …),
    //   - encounter shape (difficulty tiers — a 2-ship VeryEasy and a 4-ship
    //     Hard version of one contract are different missions).
    // Validated against SCMDB's blueprint-mission count via a diagnostic:
    // title+desc+reward = 459, +encounter = 483 (~87% of SCMDB's 558; the
    // rest is SCMDB splitting by location/standing, which we deliberately
    // don't).
    type PoolKey = (Option<LocaleKey>, Option<LocaleKey>, String, String);
    let mut groups: HashMap<PoolKey, Vec<&Mission>> = HashMap::new();
    for m in missions.iter() {
        let key = (
            m.title_key.clone(),
            m.description_key.clone(),
            reward_signature(m),
            encounter_summary(m).unwrap_or_default(),
        );
        groups.entry(key).or_default().push(m);
    }

    let mut out = Vec::with_capacity(groups.len());
    for members in groups.values() {
        // Members share title/description/rewards (same template); the first
        // is the representative. Localities are what vary → aggregated below.
        let rep = members[0];
        let r = &rep.rewards;

        let (uec_fixed, uec_calculated) = match r.uec {
            RewardAmount::Fixed(n) => (Some(n), false),
            RewardAmount::Calculated => (None, true),
            RewardAmount::None => (None, false),
        };
        let scrip = r
            .scrip
            .iter()
            .map(|s| ScripRewardView {
                name: currencies
                    .display_name(&s.currency_guid, &items, locale)
                    .map(str::to_owned),
                amount: s.amount,
            })
            .collect();
        let reputation = r
            .reputation
            .iter()
            .map(|rep| RepRewardView {
                faction_guid: rep.faction.as_ref().map(guid_string),
                amount: rep.amount,
            })
            .collect();
        let item_rewards = r
            .items
            .iter()
            .map(|it| ItemRewardView {
                entity_guid: guid_string(&it.entity_class),
                name: items
                    .name_key(&it.entity_class)
                    .and_then(|k| locale.resolve(k))
                    .map(str::to_owned),
                amount: it.amount,
            })
            .collect();

        // Blueprint rewards: union of distinct pools across members.
        let mut seen_pools = HashSet::new();
        let mut blueprint_rewards = Vec::new();
        for m in members {
            for br in &m.rewards.blueprints {
                if !seen_pools.insert(br.pool_guid) {
                    continue;
                }
                let Some(pool) = pools.get(&br.pool_guid) else {
                    continue;
                };
                let blueprints = pool
                    .items
                    .iter()
                    .map(|e| BpRewardEntry {
                        blueprint_record_guid: guid_string(&e.blueprint.blueprint_record_guid),
                        name: e.blueprint.display_name(locale).map(str::to_owned),
                        weight: e.weight,
                    })
                    .collect();
                blueprint_rewards.push(BpPoolReward {
                    pool_name: pool.name.clone(),
                    chance: br.chance,
                    blueprints,
                });
            }
        }

        // Regions: distinct locality labels across all pooled members.
        let mut seen_regions = HashSet::new();
        let mut regions = Vec::new();
        for m in members {
            for guid in &m.mission_span {
                if let Some(view) = localities.get(guid) {
                    let label = view.region_label(locale);
                    if !label.is_empty() && seen_regions.insert(label.clone()) {
                        regions.push(label);
                    }
                }
            }
        }
        regions.sort();

        out.push(MissionView {
            mission_id: guid_string(&rep.id),
            title: rep.title(locale).map(clean_mission_text),
            debug_name: rep.debug_name.clone(),
            description: rep.description(locale).map(clean_mission_text),
            once_only: rep.availability.once_only,
            shareable: rep.shareable,
            illegal: rep.illegal_flag,
            cooldown_seconds: rep
                .availability
                .cooldowns
                .completion
                .as_ref()
                .map(|d| d.mean_seconds),
            uec_fixed,
            uec_calculated,
            scrip,
            reputation,
            item_rewards,
            blueprint_rewards,
            regions,
            encounter_summary: encounter_summary(rep),
            instance_count: members.len() as u32,
        });
    }

    // Stable, readable order: by title (debug_name fallback), then id.
    out.sort_by(|a, b| {
        a.title
            .as_deref()
            .unwrap_or(&a.debug_name)
            .cmp(b.title.as_deref().unwrap_or(&b.debug_name))
            .then_with(|| a.mission_id.cmp(&b.mission_id))
    });
    out
}

/// Distinguishing reward identity for pooling. Two contracts sharing a
/// title+description but different rewards are different missions — most
/// tellingly the **reputation faction** (the giving faction), plus the BP
/// pools, scrip currencies, item unlocks, and any fixed aUEC. (Calculated
/// aUEC is engine-computed and not a stable splitter, so it's excluded.)
fn reward_signature(m: &Mission) -> String {
    let r = &m.rewards;
    let mut parts: Vec<String> = Vec::new();
    for br in &r.blueprints {
        parts.push(format!("b{}", guid_string(&br.pool_guid)));
    }
    for rep in &r.reputation {
        if let Some(f) = &rep.faction {
            parts.push(format!("r{}", guid_string(f)));
        }
    }
    for s in &r.scrip {
        parts.push(format!("s{}", guid_string(&s.currency_guid)));
    }
    for it in &r.items {
        parts.push(format!("i{}", guid_string(&it.entity_class)));
    }
    if let RewardAmount::Fixed(n) = r.uec {
        parts.push(format!("u{n}"));
    }
    parts.sort();
    parts.dedup();
    parts.join(",")
}

/// Render `~mission(Var)` runtime-substitution markers as readable `[Var]`
/// placeholders (the engine fills these per spawn; a static view can't).
fn clean_mission_text(s: &str) -> String {
    const MARK: &str = "~mission(";
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find(MARK) {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + MARK.len()..];
        match after.find(')') {
            Some(end) => {
                out.push('[');
                out.push_str(&after[..end]);
                out.push(']');
                rest = &after[end + 1..];
            }
            None => {
                // Unterminated marker — emit the remainder verbatim.
                out.push_str(&rest[pos..]);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

/// One-line encounter banner — `"2-4 ships · VeryEasy"`. `None` when the
/// mission has no ship/entity encounters and no combat class.
fn encounter_summary(m: &Mission) -> Option<String> {
    let (min, max) = m.ship_count_range();
    let class = m.combat_class();
    if max == 0 && class.is_none() {
        return None;
    }
    let mut parts = Vec::new();
    if max > 0 {
        let count = if min == max {
            max.to_string()
        } else {
            format!("{min}-{max}")
        };
        let noun = if max == 1 { "ship" } else { "ships" };
        parts.push(format!("{count} {noun}"));
    }
    if let Some(c) = class {
        parts.push(c.to_string());
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// Fill `Ingredient.name` for each ingredient by resolving its source's
/// `name_key` in the locale map. Resource ingredients resolve through the
/// `Resources` catalog (`ResourceType` GUID); item ingredients — the
/// hand-mined gems — resolve through `Items` (`EntityClassDefinition`
/// GUID). Ingredients whose GUID doesn't parse or doesn't resolve stay
/// `None`; the UI falls back to the GUID.
fn fill_ingredient_names(
    ingredients: &mut [hearth_core::Ingredient],
    resources: &Resources,
    items: &Items,
    locale: &LocaleMap,
) {
    use hearth_core::IngredientKind;
    for ing in ingredients {
        let Ok(guid) = ing.guid.parse::<sc_holotable::asset::Guid>() else {
            continue;
        };
        let name_key = match ing.kind {
            IngredientKind::Resource => resources.get(&guid).map(|r| &r.name_key),
            IngredientKind::Item => items.name_key(&guid),
        };
        if let Some(name_key) = name_key
            && let Some(name) = locale.resolve(name_key)
            && !name.is_empty()
        {
            ing.name = Some(name.to_owned());
        }
    }
}
