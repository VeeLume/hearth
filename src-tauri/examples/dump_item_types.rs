//! Throwaway diagnostic: load real SC data and tally the distinct
//! `item_type` values among craftable blueprints, with counts and a few
//! example display names per type. Used to ground the friendly-category
//! taxonomy in `src/lib/itemTypes.ts`.
//!
//! Run: `cargo run -p hearth --example dump_item_types`

use std::collections::BTreeMap;

fn main() {
    // load_inner overflows the default stack on Windows (deep generated
    // match arms) — run it on the same big-stack thread the app uses.
    let data = std::thread::Builder::new()
        .stack_size(hearth_lib::sc_loader::LOADER_STACK_SIZE)
        .spawn(|| {
            hearth_lib::sc_loader::LoadedScData::load_inner()
                .expect("load SC data (needs an SC install)")
        })
        .expect("spawn loader thread")
        .join()
        .expect("loader thread panicked");
    let bps = data.blueprints();

    // type -> (count, sample display names)
    let mut by_type: BTreeMap<String, (usize, Vec<String>)> = BTreeMap::new();
    let mut none_count = 0usize;

    for bp in &bps {
        let key = match &bp.item_type {
            Some(t) => t.clone(),
            None => {
                none_count += 1;
                continue;
            }
        };
        let entry = by_type.entry(key).or_default();
        entry.0 += 1;
        if entry.1.len() < 4 {
            entry.1.push(
                bp.display_name
                    .clone()
                    .unwrap_or_else(|| bp.blueprint_record_guid.clone()),
            );
        }
    }

    println!("\n=== item_type counts among {} blueprints ===", bps.len());
    let mut rows: Vec<_> = by_type.into_iter().collect();
    rows.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    for (ty, (count, samples)) in &rows {
        println!("{count:>5}  {ty:<28}  e.g. {}", samples.join(", "));
    }
    if none_count > 0 {
        println!("{none_count:>5}  <no item_type>");
    }
}
