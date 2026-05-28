// Maps raw CIG item classification (BpView.item_type + item_sub_type,
// e.g. "WeaponPersonal"/"Small", "Char_Armor_Helmet") into a two-level
// catalog taxonomy: a broad main category (FPS Weapons, Armor, Ship
// Components, …) with a finer subcategory (Sidearms, Helmets, Power
// Plant, …). The data layer (sc-holotable) hands us the raw enum-value
// names; the product taxonomy lives here so it's cheap to iterate
// without re-tagging the data crate.
//
// Entries are the classifications actually present among craftable
// blueprints (verified against the live datacore: 688 unique BPs).
// New types surface via the prettified "Other" fallback — add them
// here when they do.
//
// `mainOrder` sorts the broad sections; `subOrder` sorts subsections
// within a main. A `sub` of "" means the main has no subdivision — its
// items render directly under the main header (flat category).

export type Category = {
  main: string;
  mainOrder: number;
  sub: string;
  subOrder: number;
};

// item_type → static main + (optional) sub. Types that subdivide by
// SubType instead (weapons) are handled in categoryFor below.
const TABLE: Record<string, Omit<Category, "sub" | "subOrder"> & { sub: string; subOrder: number }> = {
  // ── Ship Weapons (flat — SubType is just Gun/NoseMounted; the
  //    meaningful Size axis isn't captured yet) ──
  WeaponGun: { main: "Ship Weapons", mainOrder: 11, sub: "", subOrder: 0 },

  // ── Weapon Attachments (magazines / batteries) ──
  WeaponAttachment: { main: "Weapon Attachments", mainOrder: 12, sub: "", subOrder: 0 },

  // ── Armor (subdivided by slot) ──
  Char_Armor_Undersuit: { main: "Armor", mainOrder: 20, sub: "Undersuits", subOrder: 1 },
  Char_Armor_Helmet: { main: "Armor", mainOrder: 20, sub: "Helmets", subOrder: 2 },
  Char_Armor_Torso: { main: "Armor", mainOrder: 20, sub: "Core", subOrder: 3 },
  Char_Armor_Arms: { main: "Armor", mainOrder: 20, sub: "Arms", subOrder: 4 },
  Char_Armor_Legs: { main: "Armor", mainOrder: 20, sub: "Legs", subOrder: 5 },
  Char_Armor_Feet: { main: "Armor", mainOrder: 20, sub: "Shoes", subOrder: 6 },
  Char_Armor_Backpack: { main: "Armor", mainOrder: 20, sub: "Backpacks", subOrder: 7 },

  // ── Ship Components ──
  PowerPlant: { main: "Ship Components", mainOrder: 30, sub: "Power Plant", subOrder: 1 },
  Cooler: { main: "Ship Components", mainOrder: 30, sub: "Cooler", subOrder: 2 },
  Shield: { main: "Ship Components", mainOrder: 30, sub: "Shield", subOrder: 3 },
  QuantumDrive: { main: "Ship Components", mainOrder: 30, sub: "Quantum Drive", subOrder: 4 },
  Radar: { main: "Ship Components", mainOrder: 30, sub: "Radar", subOrder: 5 },
  DockingCollar: { main: "Ship Components", mainOrder: 30, sub: "Docking Collar", subOrder: 6 },

  // ── Mining & Salvage ──
  WeaponMining: { main: "Mining & Salvage", mainOrder: 40, sub: "Mining", subOrder: 1 },
  SalvageModifier: { main: "Mining & Salvage", mainOrder: 40, sub: "Salvage", subOrder: 2 },
  SalvageHead: { main: "Mining & Salvage", mainOrder: 40, sub: "Salvage", subOrder: 2 },
  TractorBeam: { main: "Mining & Salvage", mainOrder: 40, sub: "Tractor Beam", subOrder: 3 },

  // ── Misc (test items, oddments) ──
  Misc: { main: "Misc", mainOrder: 800, sub: "", subOrder: 0 },
};

// FPS weapons (WeaponPersonal) subdivide by SubType (size class).
const FPS_SUB: Record<string, { sub: string; subOrder: number }> = {
  Small: { sub: "Sidearms", subOrder: 1 },
  Medium: { sub: "Rifles", subOrder: 2 },
  Heavy: { sub: "Heavy Weapons", subOrder: 3 },
};
const FPS_MAIN = { main: "FPS Weapons", mainOrder: 10 };

const UNMAPPED_MAIN_ORDER = 900;
const UNCATEGORIZED: Category = {
  main: "Other",
  mainOrder: UNMAPPED_MAIN_ORDER,
  sub: "Uncategorized",
  subOrder: 999,
};

/** Turn a raw type like "Char_Clothing_Torso_0" into "Char Clothing Torso 0". */
function prettify(raw: string): string {
  return raw.replace(/[_.]+/g, " ").replace(/\s+/g, " ").trim();
}

export function categoryFor(
  itemType: string | null,
  subType: string | null = null,
): Category {
  if (!itemType) return UNCATEGORIZED;

  if (itemType === "WeaponPersonal") {
    const ref = (subType ? FPS_SUB[subType] : undefined) ?? {
      sub: subType ? prettify(subType) : "Other",
      subOrder: 9,
    };
    return { ...FPS_MAIN, sub: ref.sub, subOrder: ref.subOrder };
  }

  return (
    TABLE[itemType] ?? {
      main: "Other",
      mainOrder: UNMAPPED_MAIN_ORDER,
      sub: prettify(itemType),
      subOrder: 0,
    }
  );
}
