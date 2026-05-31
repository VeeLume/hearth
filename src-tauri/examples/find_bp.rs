//! Throwaway: load the catalog and grep for blueprints whose display
//! name (or GUID) contains a substring. Used to confirm specific BPs
//! (e.g. P4-AR — the headline default-unlocked BP that was missing
//! from the pool-sourced catalog) made it into the full catalog after
//! the v0.9.0 source switch.
//!
//! Run: cargo run -p hearth --example find_bp -- p4

#[tokio::main]
async fn main() {
    let needle = std::env::args().nth(1).unwrap_or_else(|| "p4-ar".into());
    let needle_lc = needle.to_ascii_lowercase();

    let discovery = hearth_lib::sc_loader::discover()
        .await
        .expect("discover SC install");
    let bps = hearth_lib::sc_loader::build_catalog(discovery.install)
        .await
        .expect("build SC catalog");

    let mut hits = 0;
    for bp in &bps {
        let name = bp.display_name.clone().unwrap_or_default();
        if name.to_ascii_lowercase().contains(&needle_lc)
            || bp.blueprint_record_guid.to_ascii_lowercase().contains(&needle_lc)
        {
            hits += 1;
            println!(
                "{:<60}  type={:<22} sub={:<14} entity={}",
                if name.is_empty() { "<no name>" } else { &name },
                bp.item_type.as_deref().unwrap_or("—"),
                bp.item_sub_type.as_deref().unwrap_or("—"),
                bp.crafted_entity_guid.as_deref().unwrap_or("—"),
            );
        }
    }
    println!(
        "\n{} match(es) for '{}' out of {} blueprints",
        hits,
        needle,
        bps.len()
    );
}
