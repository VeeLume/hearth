// Want-item resource coverage — pure helpers that answer "do I have the
// materials to craft this?" by joining a recipe's ingredients against the
// player's live inventory (keyed by CRC in the shared data store).
//
// Quantity is the primary signal: a resource ingredient is covered when the
// summed SCU of matching stacks meets the need; an item ingredient when the
// summed count does. Quality is surfaced informationally (best available vs the
// recipe's required tier) rather than used as a hard gate — the recipe's
// `min_quality` tier and the inventory's 0..1000 quality are not known to share
// a scale, so gating on it could wrongly report "can't craft". See the
// Resources page / Wishlist for how this is shown.

import type { Ingredient, InventoryStack, Recipe } from "$lib/ipc";

export type IngredientCoverage = {
  ing: Ingredient;
  /** Whether the matching stacks meet the required quantity. */
  satisfied: boolean;
  /** False when the ingredient has no CRC (can't be matched at all). */
  tracked: boolean;
  /** Summed SCU available (resource ingredients), else null. */
  haveScu: number | null;
  /** Summed unit count available (item ingredients), else null. */
  haveCount: number | null;
  /** Best material quality (0..1000) among matching stacks, else null. */
  bestQuality: number | null;
  /** Distinct place labels holding the matching stacks. */
  locations: string[];
};

export type RecipeCoverage = {
  ingredients: IngredientCoverage[];
  /** Every ingredient's quantity is satisfied. */
  craftable: boolean;
  /** At least one ingredient could be matched (the inventory isn't empty/N/A). */
  anyTracked: boolean;
};

/** Human label for where a stack sits. */
export function stackLocationLabel(s: InventoryStack): string {
  if (s.location_name) return s.location_name;
  switch (s.location_kind) {
    case "player":
      return "On you";
    case "container":
      return "Ship / container";
    case "hangar":
      return "Hangar";
    case "location":
      return "Location";
    case "entitlement":
      return "Entitlement";
    default:
      return "Unknown";
  }
}

function coverageForIngredient(
  ing: Ingredient,
  byCrc: Map<number, InventoryStack[]>,
): IngredientCoverage {
  const base: IngredientCoverage = {
    ing,
    satisfied: false,
    tracked: ing.crc != null,
    haveScu: null,
    haveCount: null,
    bestQuality: null,
    locations: [],
  };
  if (ing.crc == null) return base;

  const stacks = byCrc.get(ing.crc) ?? [];
  const locations = [...new Set(stacks.map(stackLocationLabel))];

  if (ing.kind === "item") {
    const have = stacks.reduce((n, s) => n + (s.count ?? 0), 0);
    const need = ing.count ?? 0;
    const bestQuality = stacks.reduce<number | null>(
      (q, s) => (s.quality != null && (q == null || s.quality > q) ? s.quality : q),
      null,
    );
    return { ...base, haveCount: have, bestQuality, satisfied: have >= need && have > 0, locations };
  }

  const have = stacks.reduce((n, s) => n + (s.scu ?? 0), 0);
  const need = ing.quantity_scu ?? 0;
  const bestQuality = stacks.reduce<number | null>(
    (q, s) => (s.quality != null && (q == null || s.quality > q) ? s.quality : q),
    null,
  );
  return {
    ...base,
    haveScu: have,
    bestQuality,
    satisfied: have >= need && have > 0,
    locations,
  };
}

/** Compute per-ingredient + rollup coverage for a recipe against the inventory
 *  index. `null` recipe → no coverage. */
export function coverageFor(
  recipe: Recipe | null,
  byCrc: Map<number, InventoryStack[]>,
): RecipeCoverage | null {
  if (!recipe) return null;
  const ingredients = recipe.ingredients.map((ing) => coverageForIngredient(ing, byCrc));
  return {
    ingredients,
    craftable: ingredients.length > 0 && ingredients.every((c) => c.satisfied),
    anyTracked: ingredients.some((c) => c.tracked && (c.haveScu || c.haveCount)),
  };
}
