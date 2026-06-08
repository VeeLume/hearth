// Crafting-plan allocation — the shared-pool reservation ledger.
//
// The Crafting page used to check each recipe independently against the *whole*
// inventory, so one Iron stack "covered" every craft at once. This computes the
// honest picture instead: walk the planned crafts in priority order (Now before
// Next before Later, then creation order), spending a *mutable copy* of the
// inventory down so each stack is reserved at most once. Whatever a craft eats
// is no longer free for the ones below it.
//
// Quality is an eligibility filter, not an amount multiplier (SC fixes the
// material amount per slot): a craft with target quality Q may only draw stacks
// at quality >= Q, and we spend the lowest-eligible quality first so a Q>=500
// craft doesn't burn the Q900 stack a Q>=850 craft needs. Pure + recomputed on
// every change, so it can never drift against a re-synced inventory.

import type { CraftPlanEntry, CraftProject, InventoryStack, Recipe } from "$lib/ipc";

/** SC's global default composition quality — the "Base" an entry targets when
 *  it sets no explicit target. Matches the recipe sliders' Base. */
export const BASE_QUALITY = 500;

/** Why an ingredient couldn't be fully reserved. */
export type ShortKind = "quantity" | "quality";

/** One ingredient of a planned craft after allocation. */
export type PlanIngredient = {
  crc: number | null;
  name: string | null;
  kind: "resource" | "item";
  /** Amount needed = recipe amount × the entry's quantity. */
  need: number;
  /** Amount actually reserved from the shared pool (quality-eligible stacks). */
  reserved: number;
  /** `need − reserved`. */
  short: number;
  /** Set when `short > 0`: blocked on raw quantity, or on quality (you hold
   *  enough total but not at the target quality). */
  shortKind: ShortKind | null;
  /** The target quality this ingredient had to meet. */
  targetQuality: number;
};

export type PlanReadiness = "ready" | "partial" | "none" | "untracked" | "excluded";

/** A planned craft with its allocation result. */
export type PlanEntryResult = {
  entry: CraftPlanEntry;
  ingredients: PlanIngredient[];
  /** `ready` = every ingredient covered · `partial` = some reserved · `none` =
   *  nothing reserved · `untracked` = no inventory synced (can't tell). */
  readiness: PlanReadiness;
  /** How many ingredients are fully covered, of the total. */
  coveredCount: number;
  totalCount: number;
};

/** One material rolled up across the whole plan. */
export type MaterialRollup = {
  crc: number;
  name: string | null;
  kind: "resource" | "item";
  /** Gross amount the plan needs. */
  need: number;
  /** On-hand total (all qualities). */
  have: number;
  /** Reserved by the plan (quality-eligible draws only). */
  reserved: number;
  /** `have − reserved`, floored at 0. When `short > 0` this is exactly the
   *  sub-par stock you hold that's below the required quality (unusable). */
  free: number;
  /** `need − reserved`, floored at 0 — how much more you must acquire **at the
   *  required quality**. Captures both plain quantity gaps and quality gaps
   *  (sub-par stock doesn't reserve, so it stays "short"). */
  short: number;
  /** The binding quality floor — the highest `target_quality` among the crafts
   *  that need this material. The quality to source to. `null`/0 ⇒ any quality. */
  neededQuality: number | null;
};

export type PlanLedger = {
  entries: PlanEntryResult[];
  materials: MaterialRollup[];
  hasInventory: boolean;
};

/** Amount a stack contributes — SCU for resources, unit count for items. */
function stackAmount(s: InventoryStack): number {
  return s.scu ?? s.count ?? 0;
}

/** Compute an entry's per-ingredient need (recipe amount × quantity), without
 *  touching the pool — used for excluded entries (shown, but not reserving). */
function ingredientNeeds(recipe: Recipe | null | undefined, qty: number, target: number): PlanIngredient[] {
  return (recipe?.ingredients ?? []).map((ing) => {
    const need = (ing.kind === "item" ? (ing.count ?? 0) : (ing.quantity_scu ?? 0)) * qty;
    return {
      crc: ing.crc,
      name: ing.name,
      kind: ing.kind,
      need,
      reserved: 0,
      short: need,
      shortKind: null,
      targetQuality: target,
    };
  });
}

/** Build the reservation ledger for a plan against the inventory.
 *
 * Entries are allocated in priority order — by their project's manual order
 * (Unsorted last), then each entry's own `sort_key`. Entries in an **inactive**
 * project are excluded from both the reservation and the rollup (readiness
 * `excluded`), but still returned so the UI can show them parked.
 *
 * `recipeOf` resolves a planned entry's `blueprint_guid` to its recipe. */
export function buildPlanLedger(
  entries: CraftPlanEntry[],
  projects: CraftProject[],
  recipeOf: (blueprintGuid: string) => Recipe | null | undefined,
  inventoryByCrc: Map<number, InventoryStack[]>,
): PlanLedger {
  const hasInventory = inventoryByCrc.size > 0;

  // Project display/allocation order + active flag.
  const projOrder = new Map<string, number>();
  const projActive = new Map<string, boolean>();
  [...projects]
    .sort((a, b) => a.sort_key.localeCompare(b.sort_key))
    .forEach((p, i) => {
      projOrder.set(p.id, i);
      projActive.set(p.id, p.active);
    });
  const orderOf = (e: CraftPlanEntry) =>
    e.project_id != null ? (projOrder.get(e.project_id) ?? 1e9) : 1e9; // Unsorted last
  const isActive = (e: CraftPlanEntry) =>
    e.project_id == null ? true : (projActive.get(e.project_id) ?? true);

  // Mutable copy of the inventory, per CRC, that allocation spends down.
  const pool = new Map<number, { remaining: number; quality: number | null }[]>();
  const onHand = new Map<number, number>();
  for (const [crc, stacks] of inventoryByCrc) {
    pool.set(
      crc,
      stacks.map((s) => ({ remaining: stackAmount(s), quality: s.quality ?? null })),
    );
    onHand.set(crc, stacks.reduce((n, s) => n + stackAmount(s), 0));
  }

  // Allocation order: project order, then the entry's manual sort_key, then id.
  const active = entries
    .filter(isActive)
    .sort(
      (a, b) =>
        orderOf(a) - orderOf(b) ||
        a.sort_key.localeCompare(b.sort_key) ||
        a.id.localeCompare(b.id),
    );

  const grossNeed = new Map<number, number>();
  const reservedTotal = new Map<number, number>();
  // Binding quality floor per CRC — the highest target among the active crafts
  // that need it (the quality you must source to).
  const qualNeeded = new Map<number, number>();
  const meta = new Map<number, { name: string | null; kind: "resource" | "item" }>();
  const byId = new Map<string, PlanEntryResult>();

  for (const entry of active) {
    const recipe = recipeOf(entry.blueprint_guid);
    const target = entry.target_quality ?? BASE_QUALITY;
    const qty = Math.max(1, entry.quantity);

    const ingredients: PlanIngredient[] = (recipe?.ingredients ?? []).map((ing) => {
      const need = (ing.kind === "item" ? (ing.count ?? 0) : (ing.quantity_scu ?? 0)) * qty;
      const crc = ing.crc;
      if (crc != null) {
        grossNeed.set(crc, (grossNeed.get(crc) ?? 0) + need);
        qualNeeded.set(crc, Math.max(qualNeeded.get(crc) ?? 0, target));
        if (!meta.has(crc)) meta.set(crc, { name: ing.name, kind: ing.kind });
      }

      let reserved = 0;
      let shortKind: ShortKind | null = null;
      if (crc != null && hasInventory) {
        const stacks = pool.get(crc) ?? [];
        // Eligible = at or above target quality (unknown quality counts as
        // eligible — discrete items rarely carry one). Lowest-eligible first.
        const eligible = stacks
          .filter((s) => s.quality == null || s.quality >= target)
          .sort((a, b) => (a.quality ?? 0) - (b.quality ?? 0));
        for (const s of eligible) {
          if (reserved >= need) break;
          const draw = Math.min(s.remaining, need - reserved);
          s.remaining -= draw;
          reserved += draw;
        }
        reservedTotal.set(crc, (reservedTotal.get(crc) ?? 0) + reserved);
        if (reserved < need) {
          // Sub-par stock sitting unusable ⇒ the gap is a quality one (you hold
          // it, just not good enough); otherwise it's a plain quantity gap.
          const subPar = stacks.some(
            (s) => s.quality != null && s.quality < target && s.remaining > 0,
          );
          shortKind = subPar ? "quality" : "quantity";
        }
      }

      const short = Math.max(0, need - reserved);
      return {
        crc,
        name: ing.name,
        kind: ing.kind,
        need,
        reserved,
        short,
        shortKind: short > 0 ? shortKind : null,
        targetQuality: target,
      };
    });

    let readiness: PlanReadiness;
    if (ingredients.length === 0 || !hasInventory) readiness = "untracked";
    else if (ingredients.every((i) => i.short === 0)) readiness = "ready";
    else if (ingredients.some((i) => i.reserved > 0)) readiness = "partial";
    else readiness = "none";

    byId.set(entry.id, {
      entry,
      ingredients,
      readiness,
      coveredCount: ingredients.filter((i) => i.short === 0).length,
      totalCount: ingredients.length,
    });
  }

  // Excluded entries (inactive project): shown, but not reserved or rolled up.
  for (const entry of entries) {
    if (byId.has(entry.id)) continue;
    const ingredients = ingredientNeeds(
      recipeOf(entry.blueprint_guid),
      Math.max(1, entry.quantity),
      entry.target_quality ?? BASE_QUALITY,
    );
    byId.set(entry.id, {
      entry,
      ingredients,
      readiness: "excluded",
      coveredCount: 0,
      totalCount: ingredients.length,
    });
  }

  const materials: MaterialRollup[] = [];
  for (const [crc, need] of grossNeed) {
    const have = onHand.get(crc) ?? 0;
    const reserved = reservedTotal.get(crc) ?? 0;
    const m = meta.get(crc) ?? { name: null, kind: "resource" as const };
    materials.push({
      crc,
      name: m.name,
      kind: m.kind,
      need,
      have,
      reserved,
      free: Math.max(0, have - reserved),
      short: Math.max(0, need - reserved),
      neededQuality: qualNeeded.get(crc) ?? null,
    });
  }
  materials.sort((a, b) => (a.name ?? "").localeCompare(b.name ?? ""));

  // Return results in the original entry order (the page regroups them).
  const results = entries.map((e) => byId.get(e.id)!).filter(Boolean);
  return { entries: results, materials, hasInventory };
}
