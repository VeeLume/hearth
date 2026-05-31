// Shared craftable model — entity collapse + name helpers used by both the
// catalog (discovery) and the wishlist (fulfilment) surfaces, so they dedupe
// blueprints identically.
//
// CIG ships every armour skin / weapon paint as its own blueprint crafting
// its own entity, and sometimes several interchangeable blueprints craft the
// exact same entity. The first collapse here folds those duplicate BPs into
// one Craftable keyed on `crafted_entity_guid`. Variant/model bundling (the
// catalog's second collapse) stays in the catalog page — it's rendering-shaped
// and the wishlist doesn't need it.

import type { BpView } from "$lib/bindings";

/** A craftable entity, possibly backed by several interchangeable blueprints
 *  (same `crafted_entity_guid`). Ownership / wishlist = applies to ANY of them. */
export type Craftable = {
  /** Representative BP (shortest name) — source of name/recipe/type/family. */
  rep: BpView;
  /** Every blueprint_record_guid that crafts this entity (>= 1). */
  bpGuids: string[];
  /** Stable identity — crafted entity guid, or the BP guid when none. */
  entityKey: string;
};

/** Display name with a GUID fallback. */
export function nameOf(bp: BpView): string {
  return bp.display_name ?? bp.blueprint_record_guid;
}

/** Strip the base prefix from a variant's display name. Returns "Standard"
 *  for the variant whose name IS the base. */
export function variantSuffix(fullName: string, baseName: string): string {
  if (fullName === baseName) return "Standard";
  if (fullName.startsWith(baseName)) {
    return fullName.slice(baseName.length).trim() || "Standard";
  }
  return fullName;
}

/** Fold BPs that craft the same entity into one Craftable. */
export function collapseCraftables(items: BpView[]): Craftable[] {
  const byEntity = new Map<string, BpView[]>();
  const out: Craftable[] = [];
  for (const bp of items) {
    const key = bp.crafted_entity_guid;
    if (!key) {
      // No crafted entity to dedupe on — standalone craftable.
      out.push({
        rep: bp,
        bpGuids: [bp.blueprint_record_guid],
        entityKey: bp.blueprint_record_guid,
      });
      continue;
    }
    const arr = byEntity.get(key) ?? [];
    arr.push(bp);
    byEntity.set(key, arr);
  }
  for (const [key, arr] of byEntity) {
    const rep = arr.reduce((a, b) => (nameOf(b).length < nameOf(a).length ? b : a));
    out.push({ rep, bpGuids: arr.map((b) => b.blueprint_record_guid), entityKey: key });
  }
  return out;
}
