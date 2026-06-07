// Crafting calculator math — the client-side mirror of sc-crafting's
// quality→effect evaluation, plus display formatting. The backend
// (`get_craft_detail`) sends the raw curve bands (`ModifierRange`) and the
// property's display transform; quality is interactive (a slider), so the
// evaluation has to happen here, live, as the slider moves.

import type { ModifierRange, ModifierTransform } from "$lib/ipc";

/** The quality presets the calculator offers. (SCMDB's "Free" is a snap-vs-free
 *  slider *mode*, not a quality value — omitted until the quality bands are
 *  extracted and snapping is possible.) */
export type Preset = "min" | "base" | "half" | "max";

export const PRESETS: { id: Preset; label: string }[] = [
  { id: "min", label: "Min" },
  { id: "base", label: "Base" },
  { id: "half", label: "50%" },
  { id: "max", label: "Max" },
];

/** Quality value for a preset. `min` is per-slot (the ingredient's
 *  `min_quality`); `base` is the recipe's `default_quality`; `half` is the
 *  midpoint between Base and Max (SCMDB's "50%"), so distinct from Base. */
export function presetQuality(
  preset: Preset,
  slotMinQuality: number,
  defaultQuality: number,
): number {
  switch (preset) {
    case "min":
      return slotMinQuality;
    case "base":
      return defaultQuality;
    case "half":
      return Math.round((defaultQuality + 1000) / 2);
    case "max":
      return 1000;
  }
}

/** Linear interpolation across a band, clamped to `[start, end]`. A degenerate
 *  band (`end <= start`) yields the start value. Mirror of sc-crafting's
 *  `lerp` (lib.rs `fn lerp`). */
function lerp(startQ: number, endQ: number, vStart: number, vEnd: number, q: number): number {
  if (endQ <= startQ) return vStart;
  const qc = Math.min(Math.max(q, startQ), endQ);
  const t = (qc - startQ) / (endQ - startQ);
  return vStart + (vEnd - vStart) * t;
}

/** Evaluate one band at a quality. */
export function evalRange(r: ModifierRange, q: number): number {
  return lerp(r.start_quality, r.end_quality, r.at_start, r.at_end, q);
}

/** The result of evaluating a modifier at a quality: a factor (multiplier) or
 *  an additive bonus, per the chosen band. `null` when there are no bands. */
export type EvaluatedEffect = { value: number; additive: boolean };

/** Evaluate a modifier's bands at a quality — pick the band containing `q`,
 *  else the nearest, then interpolate. Mirror of
 *  `GameplayPropertyModifier::evaluate`. */
export function evalModifier(ranges: ModifierRange[], q: number): EvaluatedEffect | null {
  if (ranges.length === 0) return null;
  let chosen = ranges.find((r) => q >= r.start_quality && q <= r.end_quality);
  if (!chosen) {
    const dist = (r: ModifierRange) =>
      q < r.start_quality ? r.start_quality - q : q - r.end_quality;
    chosen = ranges.reduce((best, r) => (dist(r) < dist(best) ? r : best));
  }
  return { value: evalRange(chosen, q), additive: chosen.additive };
}

/** Format an evaluated effect for display: the raw factor/additive string plus
 *  an optional percent change derived from the property's display transform.
 *  Faithful to the transform — only the two factor→percent transforms yield a
 *  percent; `scale` / `value_to_factor` / `raw` / additive show no percent. */
export function formatEffect(
  transform: ModifierTransform,
  effect: EvaluatedEffect,
): { factor: string; pct: string | null } {
  if (effect.additive) {
    const sign = effect.value >= 0 ? "+" : "";
    return { factor: `${sign}${effect.value.toFixed(1)}`, pct: null };
  }
  const factor = `×${effect.value.toFixed(3)}`;
  let pct: string | null = null;
  if (transform.kind === "factor_to_percent") pct = formatPct((effect.value - 1) * 100);
  else if (transform.kind === "factor_to_negated_percent") pct = formatPct((1 - effect.value) * 100);
  return { factor, pct };
}

function formatPct(p: number): string {
  // `toFixed` already carries the minus sign for negatives.
  const sign = p > 0 ? "+" : "";
  return `${sign}${p.toFixed(2)}%`;
}

/** A short descriptor of a band's curve for the row sub-label, e.g.
 *  `Q 0–1000 · ×1.40→0.60 · Base 500`. */
export function rangeDescriptor(r: ModifierRange, defaultQuality: number): string {
  const v = (n: number) => (r.additive ? n.toFixed(0) : n.toFixed(2));
  const op = r.additive ? "+" : "×";
  return `Q ${r.start_quality}–${r.end_quality} · ${op}${v(r.at_start)}→${v(r.at_end)} · Base ${defaultQuality}`;
}
