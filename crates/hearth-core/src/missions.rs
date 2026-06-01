//! Derivations over the cooked mission list.
//!
//! Pure functions that reshape [`MissionView`]s into the other views the UI
//! needs. No I/O and no sc-holotable contact (that's `sc_data`) — this
//! operates purely on hearth-core's own domain types, so it's unit-testable
//! without a live `Datacore`.

use std::collections::HashMap;

use crate::types::{MissionRef, MissionView};

/// Invert the cooked mission list into a `blueprint_record_guid → missions`
/// map answering "which missions grant this blueprint?".
///
/// This is the reverse of [`MissionView::blueprint_rewards`] and powers the
/// wishlist's ⚐ fulfilment slot. We invert the **pooled** `MissionView`s
/// (not sc-missions' raw `missions_for_item`) so a BP links to exactly the
/// templates the Missions view shows — same ids, same titles, same pooling.
///
/// Each mission appears at most once per blueprint (a BP listed in two of one
/// mission's reward pools collapses to a single entry). The `Vec` for each
/// guid is sorted by title (then id) for a stable, readable order.
pub fn missions_by_blueprint(missions: &[MissionView]) -> HashMap<String, Vec<MissionRef>> {
    let mut map: HashMap<String, Vec<MissionRef>> = HashMap::new();
    for mission in missions {
        for pool in &mission.blueprint_rewards {
            for entry in &pool.blueprints {
                let refs = map.entry(entry.blueprint_record_guid.clone()).or_default();
                // The same BP can appear in multiple pools of one mission, but
                // it's still a single mission that grants it.
                if refs.iter().any(|r| r.mission_id == mission.mission_id) {
                    continue;
                }
                refs.push(MissionRef {
                    mission_id: mission.mission_id.clone(),
                    title: mission.title.clone(),
                    once_only: mission.once_only,
                });
            }
        }
    }
    for refs in map.values_mut() {
        refs.sort_by(|a, b| {
            a.title
                .as_deref()
                .unwrap_or(&a.mission_id)
                .cmp(b.title.as_deref().unwrap_or(&b.mission_id))
                .then_with(|| a.mission_id.cmp(&b.mission_id))
        });
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BpPoolReward, BpRewardEntry, MissionView};

    /// Build a `MissionView` carrying only the fields the inversion reads;
    /// `pools` is a list of pools, each a list of blueprint guids.
    fn mission(id: &str, title: &str, once_only: bool, pools: Vec<Vec<&str>>) -> MissionView {
        MissionView {
            mission_id: id.into(),
            title: Some(title.into()),
            debug_name: id.into(),
            description: None,
            once_only,
            shareable: false,
            illegal: false,
            cooldown_seconds: None,
            uec_fixed: None,
            uec_calculated: false,
            scrip: vec![],
            reputation: vec![],
            item_rewards: vec![],
            blueprint_rewards: pools
                .into_iter()
                .map(|guids| BpPoolReward {
                    pool_name: "pool".into(),
                    chance: 1.0,
                    blueprints: guids
                        .into_iter()
                        .map(|g| BpRewardEntry {
                            blueprint_record_guid: g.into(),
                            name: None,
                            weight: 1.0,
                        })
                        .collect(),
                })
                .collect(),
            regions: vec![],
            encounter_summary: None,
            instance_count: 1,
        }
    }

    #[test]
    fn inverts_dedupes_and_sorts() {
        let missions = vec![
            mission("m1", "Bravo", true, vec![vec!["bpA", "bpB"]]),
            // bpA appears in two of m2's pools — must collapse to one ref.
            mission("m2", "Alpha", false, vec![vec!["bpA"], vec!["bpA", "bpC"]]),
        ];
        let map = missions_by_blueprint(&missions);

        // bpA is granted by both missions, sorted by title (Alpha before Bravo).
        let a = &map["bpA"];
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].title.as_deref(), Some("Alpha"));
        assert_eq!(a[1].title.as_deref(), Some("Bravo"));
        // m2 listed bpA twice but collapses to a single entry.
        assert_eq!(a.iter().filter(|r| r.mission_id == "m2").count(), 1);

        // bpB only from m1, and the once_only flag carries through.
        assert_eq!(map["bpB"].len(), 1);
        assert_eq!(map["bpB"][0].mission_id, "m1");
        assert!(map["bpB"][0].once_only);

        // bpC only from m2.
        assert_eq!(map["bpC"].len(), 1);
        assert_eq!(map["bpC"][0].mission_id, "m2");
        assert!(!map["bpC"][0].once_only);
    }

    #[test]
    fn empty_when_no_blueprint_rewards() {
        let missions = vec![mission("m1", "No BPs", false, vec![])];
        assert!(missions_by_blueprint(&missions).is_empty());
    }
}
