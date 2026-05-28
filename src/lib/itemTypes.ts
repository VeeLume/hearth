// Maps raw CIG item-type strings (BpView.item_type, e.g.
// "Char_Armor_Helmet", "WeaponPersonal") into a two-level catalog
// taxonomy: a broad main category (Armor, Ship Components, …) with a
// finer subcategory (Helmets, Power Plant, …). The data layer
// (sc-holotable) hands us the raw enum-value names; the product taxonomy
// lives here so it's cheap to iterate without re-tagging the data crate.
//
// Entries are the item types actually present among craftable blueprints
// (verified against the live datacore, 728 BPs / patch 4.x). New types
// surface via the prettified fallback below — add them here when they do.
//
// `mainOrder` sorts the broad sections; `subOrder` sorts subsections
// within a main. Both ascending (lower first).

export type Category = {
  main: string;
  mainOrder: number;
  sub: string;
  subOrder: number;
};

const TABLE: Record<string, Category> = {
  // ── Weapons ──
  WeaponPersonal: { main: "Weapons", mainOrder: 10, sub: "FPS Weapons", subOrder: 1 },
  WeaponGun: { main: "Weapons", mainOrder: 10, sub: "Ship Weapons", subOrder: 2 },
  WeaponAttachment: { main: "Weapons", mainOrder: 10, sub: "Attachments", subOrder: 3 },

  // ── Armor ──
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

  // ── Misc (test items, oddments CIG files under "Misc") ──
  Misc: { main: "Misc", mainOrder: 800, sub: "Misc", subOrder: 1 },
};

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

export function categoryFor(itemType: string | null): Category {
  if (!itemType) return UNCATEGORIZED;
  return (
    TABLE[itemType] ?? {
      main: "Other",
      mainOrder: UNMAPPED_MAIN_ORDER,
      sub: prettify(itemType),
      subOrder: 0,
    }
  );
}
