// Maps the catalog's three classification axes (sc-crafting category +
// AttachDef item_type + sub_type) onto the two-level catalog UI taxonomy
// (main section + subsection within a main).
//
// Source axes:
//   - category_raw  — CIG-authored sc-crafting category record name with
//                     the `BlueprintCategoryRecord.` prefix stripped.
//                     SC 4.8 has 20 of these: FPSWeapons, FPSArmours,
//                     Medical, VehicleWeaponsS1-S6, plus refining /
//                     dismantle examples and a few others.
//   - item_type     — AttachDef.Type, e.g. `Char_Armor_Helmet`,
//                     `WeaponPersonal`, `PowerPlant`.
//   - item_sub_type — AttachDef.SubType, e.g. `Medium` (FPS rifle),
//                     `Magazine`, `MidRangeRadar`.
//
// The CIG category is the authoritative "what kind of craftable is this"
// axis; the AttachDef gives us slot / size info for the secondary axis.
// Different mains use different sub-axis policies (slot for armor, size
// class for FPS weapons, the size encoded in the category name for ship
// weapons, flat for medical, …) — encoded in `SubPolicy` below.
//
// `mainOrder` sorts the broad sections; `subOrder` sorts subsections
// within a main. A `sub` of `""` means the main has no subdivision and
// items render directly under the main header.

export type Category = {
  main: string;
  mainOrder: number;
  sub: string;
  subOrder: number;
};

/** How a main category derives its subsection. */
type SubPolicy =
  | "slot" // armor slot from item_type (Char_Armor_*)
  | "fpsSize" // FPS weapon size class from item_sub_type (Small/Medium/Heavy)
  | "categorySize" // size index embedded in the category name (S1..S6)
  | "shipComponent" // ship-component type from item_type (PowerPlant, Shield, …)
  | "miningSalvage" // mining/salvage sub from item_type
  | "none"; // flat — no subsection

type CategoryEntry = {
  main: string;
  mainOrder: number;
  sub: SubPolicy;
};

// Exact mapping from sc-crafting category_raw to a main + sub policy.
// Sizes S1..S6 collapse to one "Ship Weapons" main with the size as the
// sub, which keeps the catalog navigable without a long flat list of
// size-specific sections.
const BY_CATEGORY: Record<string, CategoryEntry> = {
  FPSWeapons: { main: "FPS Weapons", mainOrder: 10, sub: "fpsSize" },
  FPSArmours: { main: "Armor", mainOrder: 20, sub: "slot" },
  Medical: { main: "Medical", mainOrder: 25, sub: "none" },
  VehicleWeaponsS1: { main: "Ship Weapons", mainOrder: 30, sub: "categorySize" },
  VehicleWeaponsS2: { main: "Ship Weapons", mainOrder: 30, sub: "categorySize" },
  VehicleWeaponsS3: { main: "Ship Weapons", mainOrder: 30, sub: "categorySize" },
  VehicleWeaponsS4: { main: "Ship Weapons", mainOrder: 30, sub: "categorySize" },
  VehicleWeaponsS5: { main: "Ship Weapons", mainOrder: 30, sub: "categorySize" },
  VehicleWeaponsS6: { main: "Ship Weapons", mainOrder: 30, sub: "categorySize" },
};

// Regex-based families for categories that share a naming pattern. Less
// specific than BY_CATEGORY but cheaper to maintain than enumerating
// every numbered variant.
const FAMILY_PATTERNS: Array<{
  pattern: RegExp;
  entry: (m: RegExpMatchArray) => CategoryEntry;
}> = [
  {
    pattern: /^RefiningExample\d+$/,
    entry: () => ({ main: "Refining (Examples)", mainOrder: 700, sub: "none" }),
  },
  {
    pattern: /^DismantleExample\d+$/,
    entry: () => ({ main: "Dismantle (Examples)", mainOrder: 710, sub: "none" }),
  },
];

// Fallback sub-axis derivations from item_type / item_sub_type, used
// when we know the main but the per-category SubPolicy didn't resolve,
// or when there's no category_raw at all and we have to infer from
// item_type alone.

const ARMOR_SLOTS: Record<string, { sub: string; subOrder: number }> = {
  Char_Armor_Undersuit: { sub: "Undersuits", subOrder: 1 },
  Char_Armor_Helmet: { sub: "Helmets", subOrder: 2 },
  Char_Armor_Torso: { sub: "Core", subOrder: 3 },
  Char_Armor_Arms: { sub: "Arms", subOrder: 4 },
  Char_Armor_Legs: { sub: "Legs", subOrder: 5 },
  Char_Armor_Feet: { sub: "Shoes", subOrder: 6 },
  Char_Armor_Backpack: { sub: "Backpacks", subOrder: 7 },
};

const FPS_SIZES: Record<string, { sub: string; subOrder: number }> = {
  Small: { sub: "Sidearms", subOrder: 1 },
  Medium: { sub: "Rifles", subOrder: 2 },
  Heavy: { sub: "Heavy Weapons", subOrder: 3 },
};

const SHIP_COMPONENTS: Record<string, { sub: string; subOrder: number }> = {
  PowerPlant: { sub: "Power Plant", subOrder: 1 },
  Cooler: { sub: "Cooler", subOrder: 2 },
  Shield: { sub: "Shield", subOrder: 3 },
  QuantumDrive: { sub: "Quantum Drive", subOrder: 4 },
  Radar: { sub: "Radar", subOrder: 5 },
  DockingCollar: { sub: "Docking Collar", subOrder: 6 },
};

const MINING_SALVAGE: Record<string, { sub: string; subOrder: number }> = {
  WeaponMining: { sub: "Mining", subOrder: 1 },
  SalvageModifier: { sub: "Salvage", subOrder: 2 },
  SalvageHead: { sub: "Salvage", subOrder: 2 },
  TractorBeam: { sub: "Tractor Beam", subOrder: 3 },
};

// Used when category_raw is null/unknown — preserves the existing
// AttachDef-based grouping so unclassified blueprints still land
// somewhere reasonable.
const ITEM_TYPE_FALLBACK: Record<string, Omit<Category, "mainOrder"> & { mainOrder: number }> = {
  WeaponGun: { main: "Ship Weapons", mainOrder: 30, sub: "Uncategorized", subOrder: 99 },
  WeaponAttachment: { main: "Magazines & Batteries", mainOrder: 35, sub: "", subOrder: 0 },
  PowerPlant: { main: "Ship Components", mainOrder: 40, sub: "Power Plant", subOrder: 1 },
  Cooler: { main: "Ship Components", mainOrder: 40, sub: "Cooler", subOrder: 2 },
  Shield: { main: "Ship Components", mainOrder: 40, sub: "Shield", subOrder: 3 },
  QuantumDrive: { main: "Ship Components", mainOrder: 40, sub: "Quantum Drive", subOrder: 4 },
  Radar: { main: "Ship Components", mainOrder: 40, sub: "Radar", subOrder: 5 },
  DockingCollar: { main: "Ship Components", mainOrder: 40, sub: "Docking Collar", subOrder: 6 },
  WeaponMining: { main: "Mining & Salvage", mainOrder: 50, sub: "Mining", subOrder: 1 },
  SalvageModifier: { main: "Mining & Salvage", mainOrder: 50, sub: "Salvage", subOrder: 2 },
  SalvageHead: { main: "Mining & Salvage", mainOrder: 50, sub: "Salvage", subOrder: 2 },
  TractorBeam: { main: "Mining & Salvage", mainOrder: 50, sub: "Tractor Beam", subOrder: 3 },
  Misc: { main: "Misc", mainOrder: 800, sub: "", subOrder: 0 },
};

const UNKNOWN_MAIN_ORDER = 900;
const UNCATEGORIZED: Category = {
  main: "Other",
  mainOrder: UNKNOWN_MAIN_ORDER,
  sub: "Uncategorized",
  subOrder: 999,
};

/** Turn a raw camelCase / underscored name into a spaced label. */
function prettify(raw: string): string {
  return raw
    .replace(/[_.]+/g, " ")
    .replace(/([a-z])([A-Z])/g, "$1 $2")
    .replace(/\s+/g, " ")
    .trim();
}

function lookupCategory(raw: string): CategoryEntry | null {
  const exact = BY_CATEGORY[raw];
  if (exact) return exact;
  for (const fam of FAMILY_PATTERNS) {
    const m = raw.match(fam.pattern);
    if (m) return fam.entry(m);
  }
  return null;
}

/** Look up `key` in `table`; if missing, derive a sub from `fallbackRaw`. */
function tableOrPrettify(
  table: Record<string, { sub: string; subOrder: number }>,
  key: string | null,
  fallbackRaw: string | null,
): { sub: string; subOrder: number } {
  if (key) {
    const hit = table[key];
    if (hit) return hit;
  }
  return {
    sub: fallbackRaw ? prettify(fallbackRaw) : "Other",
    subOrder: 99,
  };
}

function resolveSub(
  policy: SubPolicy,
  categoryRaw: string | null,
  itemType: string | null,
  subType: string | null,
): { sub: string; subOrder: number } {
  switch (policy) {
    case "slot":
      return tableOrPrettify(ARMOR_SLOTS, itemType, itemType);
    case "fpsSize":
      return tableOrPrettify(FPS_SIZES, subType, subType);
    case "categorySize": {
      const m = categoryRaw?.match(/S(\d)$/);
      if (m) return { sub: `Size ${m[1]}`, subOrder: Number(m[1]) };
      return { sub: "Other", subOrder: 99 };
    }
    case "shipComponent":
      return tableOrPrettify(SHIP_COMPONENTS, itemType, itemType);
    case "miningSalvage":
      return tableOrPrettify(MINING_SALVAGE, itemType, itemType);
    case "none":
      return { sub: "", subOrder: 0 };
  }
}

/**
 * Resolve a blueprint's catalog category by combining the CIG
 * sc-crafting category (primary) with the AttachDef type/subtype
 * (secondary). Falls back to item_type-based mapping when the BP has no
 * sc-crafting category, and ultimately to "Other / Uncategorized".
 */
export function categoryFor(
  categoryRaw: string | null,
  itemType: string | null,
  subType: string | null = null,
): Category {
  // Primary path — we have a sc-crafting category and recognise it.
  if (categoryRaw) {
    const entry = lookupCategory(categoryRaw);
    if (entry) {
      const sub = resolveSub(entry.sub, categoryRaw, itemType, subType);
      return {
        main: entry.main,
        mainOrder: entry.mainOrder,
        sub: sub.sub,
        subOrder: sub.subOrder,
      };
    }
    // Unrecognised sc-crafting category — surface the prettified raw
    // name so it's discoverable but doesn't disappear into "Other".
    return {
      main: prettify(categoryRaw),
      mainOrder: UNKNOWN_MAIN_ORDER - 50,
      sub: itemType ? prettify(itemType) : "",
      subOrder: 0,
    };
  }

  // Fallback — no sc-crafting category. Use the AttachDef item_type
  // table so the BP still lands in a reasonable bucket.
  if (itemType) {
    const fb = ITEM_TYPE_FALLBACK[itemType];
    if (fb) return fb;
    return {
      main: "Other",
      mainOrder: UNKNOWN_MAIN_ORDER,
      sub: prettify(itemType),
      subOrder: 0,
    };
  }

  return UNCATEGORIZED;
}
