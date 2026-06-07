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

import type { BpView, Ingredient } from "$lib/ipc";

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

/** Format a craft time in seconds as a short human string. */
export function formatCraftTime(seconds: number | null): string {
  if (seconds == null || seconds <= 0) return "—";
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.round(seconds % 60);
  if (h > 0) return m > 0 ? `${h}h ${m}m` : `${h}h`;
  if (m > 0) return s > 0 ? `${m}m ${s}s` : `${m}m`;
  return `${s}s`;
}

/** Format an SCU quantity. Most recipe ingredients are << 1 SCU (e.g. 0.02),
 *  so default to 2 decimals; widen for larger values. */
export function formatScu(scu: number | null): string {
  if (scu == null) return "?";
  if (scu < 1) return scu.toFixed(2);
  if (scu < 10) return scu.toFixed(1);
  return scu.toFixed(0);
}

/** Format a recipe ingredient's quantity for display. Resources are bulk
 *  cargo (SCU); items — the hand-mined gems — are a discrete unit count.
 *  Returns the amount string and an optional unit label (`null` for items,
 *  whose `×N` form needs no unit). */
export function formatIngredientQty(ing: Ingredient): {
  amount: string;
  unit: string | null;
} {
  if (ing.kind === "item") {
    return { amount: `×${ing.count ?? "?"}`, unit: null };
  }
  return { amount: formatScu(ing.quantity_scu), unit: "SCU" };
}

/** Deep-link to the Missions view, pre-filtered to the missions that grant any
 *  of this craftable's interchangeable BPs (`?bp=guid,guid…`); `name` labels the
 *  banner there. Passing every interchangeable BP guid matches whichever record
 *  a mission pool happens to reference. The Missions page reads these params
 *  (`bpFilter` / `bpFilterName`). */
export function missionsLink(c: Craftable): string {
  const params = new URLSearchParams({
    bp: c.bpGuids.join(","),
    name: nameOf(c.rep),
  });
  return `/missions?${params}`;
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
