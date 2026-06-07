// Local UI preferences — reactive + persisted to localStorage. Not synced state
// (that lives in AppSettings via the backend); these are per-device view
// choices that don't belong in the DB.

import { browser } from "$app/environment";
import type { Preset } from "$lib/domain/crafting";

/** The quality a recipe's sliders start at when opened: the four presets plus
 *  "best" — the best quality you hold per material (Base for unowned). */
export type DefaultQuality = Preset | "best";

const KEY = "hearth:craftDefaultQuality";
const VALID: DefaultQuality[] = ["min", "base", "half", "max", "best"];

function load(): DefaultQuality {
  if (!browser) return "base";
  const v = localStorage.getItem(KEY) ?? "";
  return (VALID as string[]).includes(v) ? (v as DefaultQuality) : "base";
}

let _craftDefaultQuality = $state<DefaultQuality>(load());

/** Which quality recipes open at. Persisted; defaults to Base (the prior
 *  behaviour) until the user picks another. */
export const craftDefaultQuality = {
  get value(): DefaultQuality {
    return _craftDefaultQuality;
  },
  set(v: DefaultQuality) {
    _craftDefaultQuality = v;
    if (browser) localStorage.setItem(KEY, v);
  },
};

/** Selectable options, in display order, for a "default quality" picker. */
export const DEFAULT_QUALITY_OPTIONS: { id: DefaultQuality; label: string }[] = [
  { id: "min", label: "Min" },
  { id: "base", label: "Base" },
  { id: "half", label: "50%" },
  { id: "max", label: "Max" },
  { id: "best", label: "Best in stock" },
];
