//! Investigate what structural data, if any, links blueprint variants
//! to their base item. Same-recipe bundling alone produced false
//! positives (LH86 / S-38 / Salvo / Coda pistols all bundled together
//! because they share a 3-ingredient recipe) — we need a stronger
//! signal.
//!
//! For each blueprint whose display name matches the substring arg,
//! this dumps every field that could plausibly identify the base:
//!   - crafted entity record name + full file path
//!   - SItemDefinition: short_name (locale-resolved), display_type,
//!     tags string, required_tags string, manufacturer
//!   - EntityClassDefinition: category, tag GUIDs resolved through
//!     sc-tags to tag-path strings
//!
//! Run: cargo run -p hearth --example probe_variants -- pistol

use sc_holotable::asset::{
    AssetConfig, AssetData, AssetSource, Datacore, LocaleMap, RecordPaths,
    snapshot_meta_from_install,
};
use sc_holotable::asset::generated::{
    DataForgeComponentParamsPtr, EntityClassDefinition, RecordLookup, SAttachableComponentParams,
    SItemDefinition,
};
use sc_holotable::crafting::{Blueprints, Process};
use sc_holotable::items::Items;
use sc_holotable::tags::Tags;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let needle = std::env::args().nth(1).unwrap_or_else(|| "lh86".into());
    let needle_lc = needle.to_ascii_lowercase();
    let max = std::env::args()
        .nth(2)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(20);

    // Load: same path the loader uses, but skip the snapshot cache so we
    // can inspect the live Datacore for arbitrary records.
    eprintln!("(loading live SC data — this takes ~30s)");
    let mut installs = sc_holotable::install::discover()?;
    installs.sort_by_key(|i| i.channel.priority());
    let install = installs.into_iter().next().expect("no install");
    let assets = AssetSource::open(&install.data_p4k())?;
    let asset_data = AssetData::extract(&assets, &AssetConfig::minimal())?;
    let datacore = Datacore::parse(&assets, &asset_data)?;
    let locale_bytes = assets.find_and_read(|n| {
        let n = n.to_ascii_lowercase();
        n.ends_with("english\\global.ini") || n.ends_with("english/global.ini")
    })?;
    drop(assets);
    let locale = build_locale_map(&locale_bytes.expect("global.ini").1)?;

    let items = Items::build(datacore.records());
    let tags = Tags::build(datacore.records());
    let paths = RecordPaths::build(&datacore);
    let bps = Blueprints::build(&datacore, &items);

    let _ = snapshot_meta_from_install(&install);

    let mut hits = 0;
    for bp in bps.iter() {
        if !matches!(bp.process, Process::Creation { .. }) {
            continue;
        }
        let name = bp
            .display_name(&locale)
            .map(str::to_owned)
            .unwrap_or_default();
        if !name.to_ascii_lowercase().contains(&needle_lc) {
            continue;
        }
        hits += 1;
        if hits > max {
            println!("\n…stopped after {max} matches.");
            break;
        }

        println!("\n=== {} ===", name);
        println!("  blueprint guid : {}", bp.blueprint_record_guid);

        let Some(entity_guid) = bp.crafted_entity_guid() else {
            println!("  (no crafted entity)");
            continue;
        };
        println!("  entity guid    : {}", entity_guid);

        // RecordPaths: record name + file path
        if let Some(rp) = paths.get(&entity_guid) {
            println!("  entity name    : {}", rp.name);
            println!("  entity path    : {}", rp.path);
        } else {
            println!("  entity name    : <not in RecordPaths>");
        }

        // Items::short_name_key (resolved through locale)
        let short_name = items
            .short_name_key(&entity_guid)
            .and_then(|k| locale.resolve(k))
            .unwrap_or("<none>");
        println!("  short_name     : {}", short_name);

        // Direct EntityClassDefinition lookup for the raw fields we
        // haven't cooked into Items yet (tags, manufacturer, display_type).
        let store = datacore.records();
        let pools = &store.pools;
        if let Some(handle) = EntityClassDefinition::lookup(&store.records, &entity_guid)
            && let Some(ecd) = handle.get(pools)
        {
            println!("  ecd.category   : {:?}", ecd.category);
            if !ecd.tags.is_empty() {
                println!("  ecd.tags       :");
                for tag_guid in &ecd.tags {
                    let path = tag_path(&tags, tag_guid);
                    println!("    - {}", path);
                }
            } else {
                println!("  ecd.tags       : (empty)");
            }

            if let Some(item_def) = find_item_def(ecd, pools) {
                println!("  item.display_type   : {:?}", item_def.display_type);
                println!("  item.tags           : {:?}", item_def.tags);
                println!("  item.required_tags  : {:?}", item_def.required_tags);
                if let Some(mguid) = item_def.manufacturer {
                    let mname = paths
                        .get(&mguid)
                        .map(|r| r.name.as_str())
                        .unwrap_or("<unknown>");
                    println!("  item.manufacturer   : {} ({})", mname, mguid);
                } else {
                    println!("  item.manufacturer   : <none>");
                }
                println!(
                    "  item.inherit_parent_manufacturer : {}",
                    item_def.inherit_parent_manufacturer
                );
            } else {
                println!("  (no SItemDefinition on entity)");
            }
        } else {
            println!("  (entity not found in EntityClassDefinition pool)");
        }
    }

    println!("\n{} matches for '{}'", hits.min(max), needle);
    Ok(())
}

fn find_item_def<'a>(
    ecd: &EntityClassDefinition,
    pools: &'a sc_holotable::asset::DataPools,
) -> Option<&'a SItemDefinition> {
    let attachable = ecd.components.iter().find_map(|c| match c {
        DataForgeComponentParamsPtr::SAttachableComponentParams(h) => h.get(pools),
        _ => None,
    })?;
    attachable.attach_def.and_then(|h| h.get(pools))
}

fn tag_path(tags: &Tags, guid: &sc_holotable::asset::Guid) -> String {
    let path = tags.path(guid);
    if path.is_empty() {
        format!("<unknown tag {}>", guid)
    } else {
        path.join(" / ")
    }
}

fn build_locale_map(bytes: &[u8]) -> anyhow::Result<LocaleMap> {
    let (decoded, _, _) = encoding_rs::UTF_16LE.decode(bytes);
    let content = decoded.into_owned();
    let content = content.strip_prefix('\u{FEFF}').unwrap_or(&content);
    let mut map = LocaleMap::new();
    for line in content.lines() {
        if let Some(eq) = line.find('=') {
            map.set(
                sc_holotable::asset::strip_locale_metadata(&line[..eq]),
                &line[eq + 1..],
            );
        }
    }
    Ok(map)
}

fn _unused_use_of_attachable_params(_: &SAttachableComponentParams) {}
