//! Throwaway diagnostic: load real SC data and tally the distinct
//! `item_type` values among craftable blueprints, with counts and a few
//! example display names per type. Used to ground the friendly-category
//! taxonomy in `src/lib/itemTypes.ts`.
//!
//! Run: `cargo run -p hearth --example dump_item_types`

use std::collections::BTreeMap;

#[tokio::main]
async fn main() {
    let discovery = hearth_lib::sc_loader::discover()
        .await
        .expect("discover SC install");
    let bps = hearth_lib::sc_loader::build_data(discovery.install)
        .await
        .expect("build SC data")
        .blueprints;

    // (type, subtype) -> (count, sample display names)
    let mut by_type: BTreeMap<(String, String), (usize, Vec<String>)> = BTreeMap::new();
    let mut none_count = 0usize;
    let unique: std::collections::HashSet<&str> = bps
        .iter()
        .map(|b| b.blueprint_record_guid.as_str())
        .collect();

    for bp in &bps {
        let Some(ty) = bp.item_type.clone() else {
            none_count += 1;
            continue;
        };
        let sub = bp.item_sub_type.clone().unwrap_or_else(|| "—".into());
        let entry = by_type.entry((ty, sub)).or_default();
        entry.0 += 1;
        if entry.1.len() < 4 {
            entry.1.push(
                bp.display_name
                    .clone()
                    .unwrap_or_else(|| bp.blueprint_record_guid.clone()),
            );
        }
    }

    println!(
        "\n=== (item_type, sub_type) among {} rows / {} unique BPs ===",
        bps.len(),
        unique.len()
    );
    let mut rows: Vec<_> = by_type.into_iter().collect();
    rows.sort_by(|a, b| b.1.0.cmp(&a.1.0));
    for ((ty, sub), (count, samples)) in &rows {
        println!(
            "{count:>5}  {ty:<22} / {sub:<16}  e.g. {}",
            samples.join(", ")
        );
    }
    if none_count > 0 {
        println!("{none_count:>5}  <no item_type>");
    }
}
