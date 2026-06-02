//! Throwaway: fetch owned blueprints via sc-dossier and resolve them against
//! the Hearth catalog. Confirms sc-dossier works in-tree and shows which
//! sc-dossier id maps to the catalog's `blueprint_record_guid`.
//!
//! Run: cargo run -p hearth --example probe_owned

use std::collections::{HashMap, HashSet};

fn norm(s: &str) -> String {
    s.to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dossier = sc_dossier::Dossier::from_launcher("hearth-probe/0.0.1").await?;
    let owned = dossier.owned_blueprints().await?;
    println!("owned blueprints: {}", owned.len());

    let discovery = hearth_lib::sc_loader::discover().await.expect("discover SC install");
    let cooked = hearth_lib::sc_loader::build_data(discovery.install).await.expect("build data");
    let catalog = &cooked.blueprints;
    println!("catalog blueprints: {}", catalog.len());

    let bp_guids: HashSet<String> = catalog.iter().map(|b| norm(&b.blueprint_record_guid)).collect();
    let entity_guids: HashSet<String> =
        catalog.iter().filter_map(|b| b.crafted_entity_guid.as_deref().map(norm)).collect();
    let name_by_bp_guid: HashMap<String, String> = catalog
        .iter()
        .map(|b| (norm(&b.blueprint_record_guid), b.display_name.clone().unwrap_or_default()))
        .collect();

    let (mut bpid_bp, mut iclass_bp, mut iclass_entity, mut cat_bp) = (0, 0, 0, 0);
    for o in &owned {
        if bp_guids.contains(&norm(&o.blueprint_id)) { bpid_bp += 1; }
        if bp_guids.contains(&norm(&o.item_class_id)) { iclass_bp += 1; }
        if entity_guids.contains(&norm(&o.item_class_id)) { iclass_entity += 1; }
        if bp_guids.contains(&norm(&o.category_id)) { cat_bp += 1; }
    }
    println!("\nmapping hits (of {} owned):", owned.len());
    println!("  blueprint_id  -> blueprint_record_guid : {bpid_bp}");
    println!("  item_class_id -> blueprint_record_guid : {iclass_bp}");
    println!("  item_class_id -> crafted_entity_guid   : {iclass_entity}");
    println!("  category_id   -> blueprint_record_guid : {cat_bp}");

    println!("\nsample (blueprint_id -> name):");
    for o in owned.iter().take(15) {
        let name = name_by_bp_guid.get(&norm(&o.blueprint_id)).map(String::as_str).unwrap_or("<unresolved>");
        println!("  {}  {}", o.blueprint_id, name);
    }

    if let (Some(o), Some(b)) = (owned.first(), catalog.first()) {
        println!(
            "\nformat sample:\n  dossier blueprint_id = {}\n  catalog bp_guid      = {}",
            o.blueprint_id, b.blueprint_record_guid
        );
    }
    Ok(())
}
