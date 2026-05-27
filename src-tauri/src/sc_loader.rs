//! Loads SC reference data from the local game install.
//!
//! **Stage 1 status: stub.** Returns a fixed list of placeholder `BpView`
//! entries so the IPC pipeline + UI wiring can be exercised without
//! requiring SC to be installed. Stage 2 replaces `load_blueprints()`
//! with the real pipeline:
//!
//! ```text
//! 1. Discover SC install (RSI launcher log)
//! 2. AssetSource::open(Data.p4k)
//! 3. AssetData::extract → Datacore::parse
//! 4. BlueprintPoolRegistry::build(&datacore)
//! 5. Convert each (pool, item) via hearth_core::sc_data::bp_view
//! 6. Cache the loaded Datacore on AppState so we don't reload on every call
//! ```
//!
//! The stub deliberately uses GUID-shaped strings and a couple of pools so
//! the Svelte side gets data with realistic-looking shape during Stage 1
//! testing.

use hearth_core::BpView;

/// Stage 1 stub — returns placeholder data. Real loading wired in Stage 2.
pub fn load_blueprints() -> Vec<BpView> {
    vec![
        BpView {
            pool_guid: "00000000-0000-0000-0000-pool00000001".into(),
            pool_name: "Sample Pool (FPS Weapons)".into(),
            blueprint_record_guid: "00000000-0000-0000-0000-bpitem00000001".into(),
            crafted_entity_guid: Some("00000000-0000-0000-0000-entity00000001".into()),
            display_name: Some("[stub] Placeholder Rifle BP".into()),
            weight: 1.0,
        },
        BpView {
            pool_guid: "00000000-0000-0000-0000-pool00000001".into(),
            pool_name: "Sample Pool (FPS Weapons)".into(),
            blueprint_record_guid: "00000000-0000-0000-0000-bpitem00000002".into(),
            crafted_entity_guid: Some("00000000-0000-0000-0000-entity00000002".into()),
            display_name: Some("[stub] Placeholder Pistol BP".into()),
            weight: 0.5,
        },
        BpView {
            pool_guid: "00000000-0000-0000-0000-pool00000002".into(),
            pool_name: "Sample Pool (Ship Components)".into(),
            blueprint_record_guid: "00000000-0000-0000-0000-bpitem00000003".into(),
            crafted_entity_guid: None,
            display_name: Some("[stub] Placeholder Shield Component BP".into()),
            weight: 1.0,
        },
    ]
}
