<script lang="ts">
  import type { RecipeSlot } from "$lib/ipc";
  import { formatIngredientQty, formatScu } from "$lib/domain/catalog";
  import { coverageForIngredient } from "$lib/domain/inventory";
  import { evalModifier, formatEffect, rangeDescriptor } from "$lib/domain/crafting";
  import { data } from "$lib/state/data.svelte";

  let {
    slot,
    quality,
    defaultQuality,
    onQuality,
  }: {
    slot: RecipeSlot;
    quality: number;
    defaultQuality: number;
    onQuality: (q: number) => void;
  } = $props();

  const qty = $derived(formatIngredientQty(slot.ingredient));
  // Only modifiers with an evaluable quality band carry information; one with
  // empty `ranges` (no Linear/LinearIntegerAdditive band in the data) would
  // render as a dead property label — drop it, like SCMDB does.
  const effectiveMods = $derived(slot.modifiers.filter((m) => m.ranges.length > 0));

  // Resource coverage — "do I have this material?", joined against the live
  // inventory by CRC. Only meaningful once a resource sync has populated it.
  const hasInventory = $derived(data.inventory.length > 0);
  const cov = $derived(coverageForIngredient(slot.ingredient, data.inventoryByCrc));
  const haveNeed = $derived.by(() => {
    if (slot.ingredient.kind === "item") {
      return `have ×${cov.haveCount ?? 0} / need ×${slot.ingredient.count ?? 0}`;
    }
    return `have ${formatScu(cov.haveScu)} / need ${formatScu(slot.ingredient.quantity_scu)} SCU`;
  });
</script>

<div class="slot">
  <div class="slot-head">
    <span class="slot-name">{slot.slot_name ?? "Slot"}</span>
    <span class="material">{slot.ingredient.name ?? "Unknown material"}</span>
    <span class="qty">{qty.amount}{#if qty.unit}<span class="unit"> {qty.unit}</span>{/if}</span>
    {#if slot.ingredient.min_quality > 0}
      <span class="minq" title="Minimum required quality">≥ Q{slot.ingredient.min_quality}</span>
    {/if}
  </div>

  {#if hasInventory && cov.tracked}
    <div class="coverage" class:ok={cov.satisfied}>
      <span class="have">{cov.satisfied ? "✓ " : ""}{haveNeed}</span>
      {#if cov.bestQuality != null}<span class="best" title="Best available quality">best Q{cov.bestQuality}</span>{/if}
      {#if cov.locations.length}<span class="where" title="Where it's stored">@ {cov.locations.join(", ")}</span>{/if}
    </div>
  {/if}

  <div class="quality-row">
    <span class="qlabel">Quality</span>
    <input
      class="slider"
      type="range"
      min="0"
      max="1000"
      step="1"
      value={quality}
      oninput={(e) => onQuality(e.currentTarget.valueAsNumber)}
    />
    <span class="qval">{quality}</span>
  </div>

  {#if effectiveMods.length > 0}
    <ul class="modifiers">
      {#each effectiveMods as mod, i (i)}
        {@const effect = evalModifier(mod.ranges, quality)}
        {@const fx = effect ? formatEffect(mod.transform, effect) : null}
        <li class="modifier">
          <span class="mod-name">{mod.property_name ?? "Effect"}</span>
          {#if mod.ranges[0]}
            <span class="mod-curve">{rangeDescriptor(mod.ranges[0], defaultQuality)}</span>
          {/if}
          {#if fx}
            <span class="mod-value">
              <span class="factor">{fx.factor}</span>
              {#if fx.pct}<span class="pct" class:down={fx.pct.startsWith("-")}>{fx.pct}</span>{/if}
            </span>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .slot {
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--panel);
    padding: 0.8rem 1rem;
  }
  .slot-head {
    display: flex;
    align-items: baseline;
    gap: 0.6rem;
  }
  .slot-name {
    font-size: 0.72rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--ember);
  }
  .material {
    font-size: 0.92rem;
    color: var(--text);
    font-weight: 500;
  }
  .qty {
    font-size: 0.82rem;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .unit {
    font-size: 0.65rem;
    color: var(--faint);
  }
  .minq {
    margin-left: auto;
    font-size: 0.7rem;
    color: var(--muted);
    padding: 0.05rem 0.35rem;
    border: 1px solid var(--line);
    border-radius: 4px;
  }

  /* Resource coverage line — have vs need, best quality, location. */
  .coverage {
    display: flex;
    align-items: baseline;
    gap: 0.7rem;
    margin-top: 0.4rem;
    font-size: 0.72rem;
    color: var(--muted);
  }
  .coverage.ok .have {
    color: var(--good);
  }
  .coverage .best,
  .coverage .where {
    color: var(--faint);
  }

  .quality-row {
    display: flex;
    align-items: center;
    gap: 0.7rem;
    margin: 0.6rem 0 0.2rem;
  }
  .qlabel {
    font-size: 0.68rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--faint);
    flex: 0 0 auto;
  }
  .slider {
    flex: 1;
    accent-color: var(--ember);
    cursor: pointer;
  }
  .qval {
    flex: 0 0 3ch;
    text-align: right;
    font-size: 0.8rem;
    font-variant-numeric: tabular-nums;
    color: var(--text);
  }

  .modifiers {
    list-style: none;
    margin: 0.5rem 0 0;
    padding: 0.5rem 0 0;
    border-top: 1px dashed var(--line);
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }
  .modifier {
    display: flex;
    align-items: baseline;
    gap: 0.7rem;
  }
  .mod-name {
    flex: 0 0 9.5rem;
    font-size: 0.82rem;
    color: var(--text);
  }
  .mod-curve {
    flex: 1;
    font-size: 0.68rem;
    color: var(--faint);
    font-variant-numeric: tabular-nums;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mod-value {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: baseline;
    gap: 0.5rem;
    font-variant-numeric: tabular-nums;
  }
  .factor {
    font-size: 0.82rem;
    color: var(--text);
  }
  .pct {
    font-size: 0.78rem;
    color: var(--good);
  }
  .pct.down {
    color: var(--ember);
  }
</style>
