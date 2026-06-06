//! Cooking a parsed `Datacore` into Hearth's reference data — the blueprint
//! catalog and the mission browser list. Both products are built from one
//! `Datacore` parse (the cold path pays the DCB cost once) and serialized
//! whole into the processed snapshot via [`super::CookedData`].
//!
//! Each builder makes its own indices over the same datacore (cheap relative
//! to the parse), so they stay self-contained.

use std::collections::{BTreeSet, HashMap, HashSet};

use hearth_core::sc_data::guid_string;
use hearth_core::{
    BpPoolReward, BpRewardEntry, BpView, DifficultyView, EncounterView, FactionView,
    ItemRewardView, MissionCategoryView, MissionRef, MissionView, PayoutView, PlaceView, RegionView,
    RepRequirementView, RepRewardView, ScripRewardView, ShipSlotView, WaveView,
};
use sc_holotable::asset::{Datacore, LocaleKey, LocaleMap, RecordPaths, class_crc};
use sc_holotable::crafting::{Blueprints, Categories, Process};
use sc_holotable::items::{ItemCatalog, Items};
// NB: `sc_holotable::locations::Locations` (the typed universe index) is named
// `Locations` and so is `sc_missions::Locations`; we only use the former here.
use sc_holotable::locations::Locations;
use sc_holotable::missions::{Encounter, Mission, Missions, PrereqView, RewardAmount, RewardCurrencies};
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

    // Collapse raw expansions into displayed missions. The key is the
    // player-meaningful identity: title + description + reward identity (which
    // BPs / faction / scrip / item kinds) + **payout variant** (difficulty
    // levels + buy-in + time — the visible aUEC the player differentiates by).
    // Location and encounter are *not* in the key: they're aggregated into the
    // entry as facets the UI groups / sub-splits. The frontend pools these
    // further (by reward identity, sub-split by system) per the consumer-
    // decides-grouping model.
    type PoolKey = (Option<LocaleKey>, Option<LocaleKey>, String, String);
    let mut groups: HashMap<PoolKey, Vec<&Mission>> = HashMap::new();
    for m in missions.iter() {
        let key = (
            m.title_key.clone(),
            m.description_key.clone(),
            reward_signature(m),
            payout_signature(m),
        );
        groups.entry(key).or_default().push(m);
    }

    let mut out = Vec::with_capacity(groups.len());
    for members in groups.values() {
        // Members share title/description/rewards/payout (same entry); the
        // first is the representative. Localities are what vary → aggregated.
        let rep = members[0];
        let r = &rep.rewards;

        let payout = PayoutView {
            calculated: matches!(r.uec, RewardAmount::Calculated),
            fixed: match r.uec {
                RewardAmount::Fixed(n) => Some(n),
                _ => None,
            },
            estimate: estimate_uec(rep),
            buy_in: rep.buy_in,
            time_to_complete: rep.time_to_complete,
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

        out.push(MissionView {
            mission_id: guid_string(&rep.id),
            title: missions.title_text(rep, locale),
            debug_name: rep.debug_name.clone(),
            description: missions.description_text(rep, locale),
            category: build_category(rep, &missions, locale),
            faction: build_faction(rep, &missions, locale),
            difficulty: rep.difficulty.map(|d| DifficultyView {
                mechanical_skill: d.mechanical_skill,
                mental_load: d.mental_load,
                risk_of_loss: d.risk_of_loss,
                game_knowledge: d.game_knowledge,
            }),
            payout,
            once_only: rep.availability.once_only,
            shareable: rep.shareable,
            illegal: rep.illegal_flag,
            cooldown_seconds: rep
                .availability
                .cooldowns
                .completion
                .as_ref()
                .map(|d| d.mean_seconds),
            scrip,
            reputation,
            item_rewards,
            blueprint_rewards,
            rep_required: build_rep_required(rep, &missions, locale),
            chain_required: build_chain(rep, &missions, locale),
            locations: build_locations(members, &missions, locale),
            encounters: build_encounters(rep, &missions, &items, locale),
            placeholders: missions.unresolved_markers(rep, locale),
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

/// Distinguishing reward **identity** for pooling — the *kinds* of payoff, not
/// amounts: BP pools, reputation faction, scrip currencies, item unlocks. Two
/// contracts sharing a title+description but different reward identity are
/// different missions (e.g. Region A/B vs C/D blueprint pools).
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
    parts.sort();
    parts.dedup();
    parts.join(",")
}

/// Payout-variant signature — the visible aUEC differentiator. Driven by the
/// difficulty profile (the hidden axis players don't see) plus buy-in + time.
/// Same payout inputs ⇒ same displayed reward, so these collapse; different
/// inputs split (the "harder ⇒ bigger payout" rows). Independent of the (still
/// unported) reward formula — splitting on the inputs is correct regardless.
fn payout_signature(m: &Mission) -> String {
    let d = m
        .difficulty
        .map(|d| {
            format!(
                "{}-{}-{}-{}",
                d.mechanical_skill, d.mental_load, d.risk_of_loss, d.game_knowledge
            )
        })
        .unwrap_or_default();
    match m.rewards.uec {
        RewardAmount::Fixed(n) => format!("f{n}"),
        _ => format!("{d}|b{}|t{}", m.buy_in, m.time_to_complete),
    }
}

/// Estimate the engine-calculated aUEC payout for a `Calculated` reward.
///
/// The engine pays out **exponentially** in difficulty: the per-minute reward
/// rate grows by a constant factor (~1.354×) per difficulty level, not
/// linearly. The model is
///
/// ```text
/// aUEC ≈ round₂₅₀( 1232 × 1.354^weighted_difficulty × time_minutes )
/// ```
///
/// where `weighted_difficulty` is the dot product of the four difficulty axis
/// levels with their per-profile weights, and time is in **minutes**. Fitted
/// against 13 known SCMDB payouts spanning difficulty levels 2–5 — it
/// reproduces every one exactly after the 250-aUEC rounding (max error 0.0%).
/// (An earlier `1035 × weighted × time` linear model was a tangent that only
/// matched at level 4; the two extra low/high-difficulty samples revealed the
/// curve. There is no floor term — payout stays strictly proportional to time.)
///
/// Returns `None` for fixed/absent payouts or when difficulty inputs are
/// missing (no profile weights ⇒ can't weight the axes). Cross-system distance
/// scaling is runtime-derived and intentionally not modelled here.
fn estimate_uec(m: &Mission) -> Option<i32> {
    if !matches!(m.rewards.uec, RewardAmount::Calculated) {
        return None;
    }
    let d = m.difficulty?;
    let w = d.weights?;
    let weighted = d.mechanical_skill as f32 * w[0]
        + d.mental_load as f32 * w[1]
        + d.risk_of_loss as f32 * w[2]
        + d.game_knowledge as f32 * w[3];
    if weighted <= 0.0 || m.time_to_complete <= 0.0 {
        return None;
    }
    let raw = 1232.0 * 1.354_f32.powf(weighted) * m.time_to_complete;
    Some(((raw / 250.0).round() * 250.0) as i32)
}

/// Resolve the mission category (`MissionType`) name + icon.
fn build_category(m: &Mission, missions: &Missions, locale: &LocaleMap) -> Option<MissionCategoryView> {
    let info = missions.mission_types.get(&m.category?)?;
    Some(MissionCategoryView {
        name: locale.resolve(&info.name_key).map(str::to_owned),
        icon: info.icon_name.clone(),
    })
}

/// Resolve the mission's reputation faction → display name (+ stable guid key).
fn build_faction(m: &Mission, missions: &Missions, locale: &LocaleMap) -> Option<FactionView> {
    let guid = m.faction?;
    let name = missions
        .factions
        .get(&guid)
        .and_then(|f| locale.resolve(&f.display_name_key))
        .map(str::to_owned);
    Some(FactionView {
        guid: guid_string(&guid),
        name,
    })
}

/// Resolve the rep-acceptance requirements (faction + standing-tier window).
/// Includes career-contract rep gates (handler faction + contract standing),
/// which sc-missions now surfaces as synthetic reputation prereqs.
fn build_rep_required(
    m: &Mission,
    missions: &Missions,
    locale: &LocaleMap,
) -> Vec<RepRequirementView> {
    let standing = |g: &Option<sc_holotable::asset::Guid>| {
        g.as_ref()
            .and_then(|g| missions.rep_standings.get(g))
            .and_then(|s| locale.resolve(&s.display_name_key))
            .map(str::to_owned)
    };
    // Tier index parsed from the standing record name —
    // `ReputationStanding_FactionRep_Rank2` → 2.
    let rank_index = |g: &Option<sc_holotable::asset::Guid>| {
        g.as_ref()
            .and_then(|g| missions.rep_standings.get(g))
            .and_then(|s| s.record_name.rsplit("Rank").next()?.parse::<i32>().ok())
    };
    m.prerequisites
        .iter()
        .filter_map(|p| match p {
            PrereqView::Reputation {
                faction,
                min_standing,
                max_standing,
                exclude,
                ..
            } => Some(RepRequirementView {
                faction: faction
                    .as_ref()
                    .and_then(|g| missions.factions.get(g))
                    .and_then(|f| locale.resolve(&f.display_name_key))
                    .map(str::to_owned),
                min_rank: standing(min_standing),
                max_rank: standing(max_standing),
                min_rank_index: rank_index(min_standing),
                max_rank_index: rank_index(max_standing),
                exclude: *exclude,
            }),
            _ => None,
        })
        .collect()
}

/// Resolve the chain gate — prerequisite missions, deduped by title.
fn build_chain(m: &Mission, missions: &Missions, locale: &LocaleMap) -> Vec<MissionRef> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for id in missions.prerequisite_missions(m) {
        let Some(grantor) = missions.get(id) else {
            continue;
        };
        let title = missions.title_text(grantor, locale);
        let dedupe_key = title.clone().unwrap_or_else(|| guid_string(&grantor.id));
        if seen.insert(dedupe_key) {
            out.push(MissionRef {
                mission_id: guid_string(&grantor.id),
                title,
                once_only: grantor.availability.once_only,
            });
        }
    }
    out
}

/// Builds the per-locality "available in" cards across all pooled members. Each
/// `MissionLocality` the mission is offered at becomes one [`RegionView`] (a
/// parent card — *Stanton — Hurston*, *Pyro — Region A*), carrying its places
/// with their typed `LocationKind`. Localities are deduped across members.
fn build_locations(members: &[&Mission], missions: &Missions, locale: &LocaleMap) -> Vec<RegionView> {
    let mut seen_loc: HashSet<sc_holotable::asset::Guid> = HashSet::new();
    let mut out: Vec<RegionView> = Vec::new();
    for m in members {
        for guid in &m.mission_span {
            if !seen_loc.insert(*guid) {
                continue;
            }
            let Some(view) = missions.localities.get(guid) else {
                continue;
            };
            let system = view
                .systems
                .iter()
                .next()
                .map(|s| s.display().to_string())
                .unwrap_or_default();
            // Dedupe places by record name within the locality.
            let mut seen_place: HashSet<String> = HashSet::new();
            let mut places = Vec::new();
            for loc in &view.locations {
                if !seen_place.insert(loc.record_name.clone()) {
                    continue;
                }
                places.push(PlaceView {
                    name: loc.display_name(locale).map(str::to_owned),
                    record_name: loc.record_name.clone(),
                    kind: loc.kind.as_ref().map(|k| k.as_dcb_str().to_string()),
                });
            }
            // Prefer the planet's name when the locality wraps a single
            // planet — Stanton localities are record-named `Stanton1`..`Stanton4`
            // but a player knows them as Hurston / Crusader / ArcCorp /
            // microTech. Keep the cleaned region name for multi-planet
            // localities (Pyro `RegionA` spans several planets).
            let planets: Vec<&str> = places
                .iter()
                .filter(|p| p.kind.as_deref() == Some("Planet"))
                .filter_map(|p| p.name.as_deref())
                .collect();
            let name = if planets.len() == 1 {
                planets[0].to_string()
            } else {
                clean_locality_name(&view.name)
            };
            out.push(RegionView {
                system,
                name,
                places,
            });
        }
    }
    out.sort_by(|a, b| a.system.cmp(&b.system).then_with(|| a.name.cmp(&b.name)));
    out
}

/// Insert spaces at lower→upper / lower→digit boundaries so a locality record
/// stem reads as a label (`"RegionA"` → `"Region A"`).
fn clean_locality_name(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    let mut prev_lower = false;
    for ch in s.chars() {
        if prev_lower && (ch.is_uppercase() || ch.is_ascii_digit()) {
            out.push(' ');
        }
        out.push(ch);
        prev_lower = ch.is_lowercase();
    }
    out
}

/// Structured ship encounters — waves → slots (ship candidates + counts +
/// factions) + resolved cargo. NPC / entity encounters are skipped for now.
fn build_encounters(
    m: &Mission,
    missions: &Missions,
    items: &Items,
    locale: &LocaleMap,
) -> Vec<EncounterView> {
    let tree = &missions.tag_tree;
    let difficulty = m.combat_class().map(str::to_owned);
    let mut out = Vec::new();
    for enc in &m.encounters {
        let Encounter::Ships(s) = enc else { continue };
        let mut waves = Vec::new();
        for phase in &s.phases {
            let mut ships = Vec::new();
            let mut cargo: BTreeSet<String> = BTreeSet::new();
            for group in &phase.groups {
                let mut ship_names: BTreeSet<String> = BTreeSet::new();
                let mut factions: BTreeSet<String> = BTreeSet::new();
                for opt in &group.options {
                    for c in &opt.candidates {
                        if let Some(name) =
                            missions.ships.display_name(&c.entity_guid, items, locale)
                        {
                            ship_names.insert(name.to_string());
                        }
                    }
                    for f in opt.positive.factions(tree) {
                        factions.insert(f.to_string());
                    }
                    for cg in opt.positive.cargo(tree) {
                        cargo.insert(cg.to_string());
                    }
                }
                ships.push(ShipSlotView {
                    count_min: group.concurrent_range.0,
                    count_max: group.concurrent_range.1,
                    ships: ship_names.into_iter().collect(),
                    factions: factions.into_iter().collect(),
                });
            }
            waves.push(WaveView {
                name: phase.name.clone(),
                ships,
                cargo: cargo.into_iter().collect(),
            });
        }
        if !waves.is_empty() {
            out.push(EncounterView {
                label: s.variable_name.clone(),
                difficulty: difficulty.clone(),
                waves,
            });
        }
    }
    out
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
