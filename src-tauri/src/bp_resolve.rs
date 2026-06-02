//! Resolve a received-blueprint display name (from `Game.log` or the langpatch
//! UI string) to its catalog `blueprint_record_guid`s. Shared by the history
//! import and the live Game.log sensor.

use std::collections::HashMap;

use hearth_core::BpView;

/// Catalog display-name → `blueprint_record_guid`s. One name can map to
/// several interchangeable BPs (variants / duplicate-BP collapse); callers
/// mark all of them, consistent with entity-level ownership.
pub(crate) fn build_name_index(catalog: &[BpView]) -> HashMap<String, Vec<String>> {
    let mut index: HashMap<String, Vec<String>> = HashMap::new();
    for bp in catalog {
        if let Some(name) = &bp.display_name {
            let key = normalize_bp_name(name);
            if !key.is_empty() {
                index.entry(key).or_default().push(bp.blueprint_record_guid.clone());
            }
        }
    }
    index
}

fn normalize_bp_name(name: &str) -> String {
    name.trim().to_lowercase()
}

/// Resolve a received-blueprint display name to its catalog
/// `blueprint_record_guid`s.
///
/// The log carries the in-game UI string, which is **sc-langpatch's patched
/// name** when installed: its modules *add* tokens (a ship-weapon size `"S3 …"`,
/// a manufacturer-grade code `"IND2B …"`), while Hearth's catalog has the
/// vanilla p4k name. Because those edits only *add* whole tokens, the vanilla
/// name is a contiguous whole-word run inside the log name.
///
/// Strategy: exact normalized match first; then the **longest contiguous
/// whole-word run** of the log name that is itself a catalog name. Whole-word
/// alignment avoids `"Bolt"` matching inside `"Deadbolt"`; longest wins so the
/// most specific name is picked; an ambiguous tie resolves to nothing rather
/// than guess.
pub(crate) fn resolve_blueprint_guids<'a>(
    index: &'a HashMap<String, Vec<String>>,
    name: &str,
) -> Option<&'a Vec<String>> {
    let norm = normalize_bp_name(name);
    if let Some(guids) = index.get(&norm) {
        return Some(guids);
    }
    let words: Vec<&str> = norm.split_whitespace().collect();
    let mut best: Option<(usize, &'a Vec<String>)> = None;
    let mut ambiguous = false;
    for start in 0..words.len() {
        // Longest run from this start first; the full-length run equals `norm`
        // which already missed, so sub-runs are what we test.
        for end in (start + 1..=words.len()).rev() {
            let candidate = words[start..end].join(" ");
            let Some(guids) = index.get(&candidate) else { continue };
            let len = end - start;
            match best {
                Some((best_len, _)) if len > best_len => {
                    best = Some((len, guids));
                    ambiguous = false;
                }
                Some((best_len, best_guids)) if len == best_len && !std::ptr::eq(best_guids, guids) => {
                    ambiguous = true;
                }
                None => best = Some((len, guids)),
                _ => {}
            }
        }
    }
    if ambiguous {
        return None;
    }
    best.map(|(_, guids)| guids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_exact_and_additive_langpatch_edits() {
        let mut index: HashMap<String, Vec<String>> = HashMap::new();
        index.insert("aril arms".into(), vec!["g-aril".into()]);
        index.insert("attrition-3 repeater".into(), vec!["g-attr".into()]);
        index.insert("citadel".into(), vec!["g-cit".into()]);
        index.insert("deadbolt iii cannon".into(), vec!["g-dead".into()]);
        index.insert("bolt".into(), vec!["g-bolt".into()]);

        // Exact (FPS gear — sc-langpatch doesn't prefix these).
        assert_eq!(resolve_blueprint_guids(&index, "Aril Arms"), Some(&vec!["g-aril".to_string()]));
        // Added ship-weapon size token → longest whole-word run matches.
        assert_eq!(
            resolve_blueprint_guids(&index, "S3 Attrition-3 Repeater"),
            Some(&vec!["g-attr".to_string()])
        );
        // Added manufacturer-grade token.
        assert_eq!(resolve_blueprint_guids(&index, "IND2B Citadel"), Some(&vec!["g-cit".to_string()]));
        // Whole-word alignment: "Bolt" must NOT match inside "Deadbolt".
        assert_eq!(
            resolve_blueprint_guids(&index, "S3 Deadbolt III Cannon"),
            Some(&vec!["g-dead".to_string()])
        );
        // Genuine miss.
        assert!(resolve_blueprint_guids(&index, "Totally Unknown").is_none());
    }
}
