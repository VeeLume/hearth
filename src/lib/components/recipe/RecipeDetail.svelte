<script lang="ts">
  import { onMount } from "svelte";
  import type { CraftDetail, MissionRef, WishIntent } from "$lib/ipc";
  import type { Craftable } from "$lib/domain/catalog";
  import { nameOf, formatCraftTime, missionsLink } from "$lib/domain/catalog";
  import { categoryFor } from "$lib/domain/categories";
  import { coverageFor, coverageForIngredient } from "$lib/domain/inventory";
  import { PRESETS, presetQuality, type Preset } from "$lib/domain/crafting";
  import {
    craftDefaultQuality,
    DEFAULT_QUALITY_OPTIONS,
    type DefaultQuality,
  } from "$lib/state/prefs.svelte";
  import {
    data,
    owned,
    wishSet,
    toggleOwned,
    toggleWishlist,
    ensureCraftDetail,
    ensureInventory,
  } from "$lib/state/data.svelte";
  import SlotPanel from "./SlotPanel.svelte";

  let { craftable, backHref }: { craftable: Craftable; backHref: string } = $props();

  // Load the resource inventory so per-slot coverage shows wherever the detail is
  // opened from (catalog / planner / wishlist), not only where the list happened
  // to fetch it. No-op once loaded; harmless when sync is off (stays empty).
  onMount(() => {
    ensureInventory();
  });

  const cat = $derived(
    categoryFor(craftable.rep.category_raw, craftable.rep.item_type, craftable.rep.item_sub_type),
  );

  // Rich detail, fetched (cached) per blueprint. The cleanup cancels a stale
  // fetch when the selected craftable changes mid-flight.
  let detail = $state<CraftDetail | null>(null);
  let detailLoading = $state(false);
  $effect(() => {
    const guid = craftable.rep.blueprint_record_guid;
    let cancelled = false;
    detail = null;
    detailLoading = true;
    ensureCraftDetail(guid).then((d) => {
      if (cancelled) return;
      detail = d;
      detailLoading = false;
    });
    return () => {
      cancelled = true;
    };
  });

  const hasInventory = $derived(data.inventory.length > 0);

  // Quality each slot takes for a given preference. "best" reads the live
  // inventory (best owned quality per material, Base for unowned).
  function qualitiesFor(d: CraftDetail, pref: DefaultQuality): number[] {
    if (pref === "best") {
      return d.slots.map(
        (s) =>
          coverageForIngredient(s.ingredient, data.inventoryByCrc).bestQuality ?? d.default_quality,
      );
    }
    return d.slots.map((s) => presetQuality(pref, s.ingredient.min_quality, d.default_quality));
  }

  // Per-slot quality, initialised to the user's default-quality preference
  // whenever a new recipe loads (or the preference changes). With "best", the
  // effect also tracks the inventory, so it re-applies when a sync lands.
  let qualities = $state<number[]>([]);
  $effect(() => {
    const d = detail;
    const pref = craftDefaultQuality.value;
    qualities = d ? qualitiesFor(d, pref) : [];
  });

  function applyPreset(preset: Preset) {
    const d = detail;
    if (d) qualities = qualitiesFor(d, preset);
  }
  function applyBestInStock() {
    const d = detail;
    if (d) qualities = qualitiesFor(d, "best");
  }

  // Entity-level ownership / wishlist (all interchangeable BPs together).
  const isOwned = $derived(craftable.bpGuids.some((g) => owned.has(g)));
  const wantBp = $derived(craftable.bpGuids.some((g) => wishSet("recipe").has(g)));
  const wantItem = $derived(craftable.bpGuids.some((g) => wishSet("item").has(g)));
  let actionError = $state<string | null>(null);
  async function toggleOwnedEntity() {
    const ownedGuids = craftable.bpGuids.filter((g) => owned.has(g));
    const targets = ownedGuids.length > 0 ? ownedGuids : craftable.bpGuids;
    for (const g of targets) {
      const e = await toggleOwned(g);
      if (e) actionError = e;
    }
  }
  async function toggleWishEntity(intent: WishIntent) {
    const set = wishSet(intent);
    const wanted = craftable.bpGuids.filter((g) => set.has(g));
    const targets = wanted.length > 0 ? wanted : craftable.bpGuids;
    for (const g of targets) {
      const e = await toggleWishlist(g, intent);
      if (e) actionError = e;
    }
  }

  // Missions granting any interchangeable BP, deduped + sorted.
  const missions = $derived.by((): MissionRef[] => {
    const seen = new Set<string>();
    const out: MissionRef[] = [];
    for (const g of craftable.bpGuids)
      for (const m of data.grantedBy[g] ?? []) {
        if (seen.has(m.mission_id)) continue;
        seen.add(m.mission_id);
        out.push(m);
      }
    out.sort((a, b) => (a.title ?? a.mission_id).localeCompare(b.title ?? b.mission_id));
    return out;
  });

  // The gameplay properties this recipe's materials reshape — the stats whose
  // absolute values the (not-yet-bound) Product Stats panel will show. Distinct,
  // by display name, over slots' effective modifiers.
  const affectedProps = $derived.by(() => {
    if (!detail) return [];
    const set = new Set<string>();
    for (const s of detail.slots)
      for (const m of s.modifiers) if (m.property_name && m.ranges.length > 0) set.add(m.property_name);
    return [...set].sort();
  });

  // Have-materials rollup (only with a synced inventory).
  const rollup = $derived.by(() => {
    if (data.inventory.length === 0) return null;
    const c = coverageFor(craftable.rep.recipe, data.inventoryByCrc);
    if (!c) return null;
    if (c.craftable) return { label: "✓ have materials", ready: true };
    const have = c.ingredients.filter((i) => i.satisfied).length;
    return { label: `materials ${have}/${c.ingredients.length}`, ready: false };
  });
</script>

<article class="detail">
  <a class="back" href={backHref}>← Back to list</a>

  <header class="head">
    <div class="title-row">
      <h2 class="name">{nameOf(craftable.rep)}</h2>
      {#if craftable.bpGuids.length > 1}
        <span class="dup-tag" title="{craftable.bpGuids.length} interchangeable blueprints craft this item">
          {craftable.bpGuids.length} BPs
        </span>
      {/if}
      {#if rollup}
        <span class="rollup" class:ready={rollup.ready} title="Whether your synced resource inventory covers this recipe">
          {rollup.label}
        </span>
      {/if}
    </div>
    <div class="badges">
      <span class="badge main">{cat.main}</span>
      {#if cat.sub}<span class="badge">{cat.sub}</span>{/if}
    </div>

    <div class="actions">
      <button class="own-toggle" class:on={isOwned} onclick={toggleOwnedEntity}
        title={isOwned ? "Blueprint owned — click to unmark" : "Mark blueprint owned"}>
        <span class="check">{isOwned ? "✓" : ""}</span>
        {isOwned ? "Owned" : "Mark owned"}
      </button>
      {#if !isOwned}
        <button class="wish" class:on={wantBp} onclick={() => toggleWishEntity("recipe")}
          title={wantBp ? "Remove blueprint from wishlist" : "Want blueprint (acquire via mission rewards)"}>
          ⚐ Want BP
        </button>
      {/if}
      <button class="wish" class:on={wantItem} onclick={() => toggleWishEntity("item")}
        title={wantItem ? "Remove item from wishlist" : "Want a crafted copy of this item"}>
        ♡ Want item
      </button>
    </div>
    {#if actionError}<p class="action-error">{actionError}</p>{/if}
  </header>

  {#if detailLoading}
    <p class="status">Loading recipe…</p>
  {:else if !detail}
    <p class="status">No recipe data for this blueprint.</p>
  {:else}
    {#if detail.slots.length > 0}
      <div class="presets">
        <span class="presets-label">Quality</span>
        {#each PRESETS as p (p.id)}
          <button class="preset" onclick={() => applyPreset(p.id)}>{p.label}</button>
        {/each}
        {#if hasInventory}
          <button
            class="preset"
            onclick={applyBestInStock}
            title="Set each slot to the best quality you have in stock — Base for materials you don't hold"
          >Best in stock</button>
        {/if}
        <label class="default-pick" title="Quality each slot starts at when you open a recipe">
          Open at
          <select
            value={craftDefaultQuality.value}
            onchange={(e) => craftDefaultQuality.set(e.currentTarget.value as DefaultQuality)}
          >
            {#each DEFAULT_QUALITY_OPTIONS as o (o.id)}
              <option value={o.id}>{o.label}</option>
            {/each}
          </select>
        </label>
      </div>
    {/if}

    <!-- Two columns on wide screens (materials | stats & missions), stacked
         when narrow — see the container query in <style>. -->
    <div class="recipe-body">
      <div class="col-main">
        {#if detail.slots.length > 0}
          <div class="slots">
            {#each detail.slots as slot, i (i)}
              <SlotPanel
                {slot}
                quality={qualities[i] ?? detail.default_quality}
                defaultQuality={detail.default_quality}
                onQuality={(q) => (qualities[i] = q)}
              />
            {/each}
          </div>
        {:else}
          <p class="status">This recipe has no material slots.</p>
        {/if}

        <div class="craft-time">
          <span class="ct-label">Craft Time</span>
          <span class="ct-value">⏱ {formatCraftTime(detail.craft_time_seconds)}</span>
        </div>
      </div>

      <div class="col-side">
        <!-- Product Stats — placeholder until the GPP → base-stat-field binding
             lands. We model the per-material modifiers (shown per slot), but not
             yet the crafted item's absolute final stats. -->
        <div class="product-stats">
          <div class="ps-head">
            <h3>Product Stats</h3>
            <span class="ps-soon">soon</span>
          </div>
          <p class="ps-note">
            The crafted item's final stats — its base values reshaped by each
            material's quality. The per-material modifiers are modelled (left);
            binding them to the item's base stats is still in progress.
          </p>
          {#if affectedProps.length > 0}
            <ul class="ps-list">
              {#each affectedProps as p (p)}
                <li><span class="ps-prop">{p}</span><span class="ps-val">—</span></li>
              {/each}
            </ul>
          {/if}
        </div>

        {#if missions.length > 0}
          <section class="missions">
            <h3>Missions ({missions.length})</h3>
            <a class="missions-link" href={missionsLink(craftable)}>
              View the {missions.length} mission{missions.length === 1 ? "" : "s"} that grant this →
            </a>
            <ul>
              {#each missions as m (m.mission_id)}
                <li>
                  <span class="mission">
                    {m.title ?? m.mission_id}
                    {#if m.once_only}<span class="once" title="Non-repeatable reward">once</span>{/if}
                  </span>
                </li>
              {/each}
            </ul>
          </section>
        {/if}
      </div>
    </div>
  {/if}
</article>

<style>
  .detail {
    max-width: 1400px;
    /* Query container so the body can go two-column on actual available width
       (not viewport) — robust to the sidebar + window size. */
    container-type: inline-size;
  }

  /* Body: stacked by default; two columns once there's room. */
  .recipe-body {
    display: grid;
    grid-template-columns: 1fr;
    gap: 1rem;
    align-items: start;
  }
  @container (min-width: 900px) {
    .recipe-body {
      grid-template-columns: minmax(0, 1.5fr) minmax(0, 1fr);
    }
  }
  .col-main,
  .col-side {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    min-width: 0;
  }

  /* Product Stats placeholder — the absolute final stats, pending the
     gameplay-property → base-stat binding. */
  .product-stats {
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--panel);
    padding: 0.9rem 1rem;
  }
  .ps-head {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .ps-head h3 {
    margin: 0;
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--ember);
  }
  .ps-soon {
    font-size: 0.6rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--warn);
    border: 1px solid var(--line);
    border-radius: 999px;
    padding: 0.05rem 0.45rem;
  }
  .ps-note {
    margin: 0.5rem 0 0;
    font-size: 0.78rem;
    color: var(--muted);
    line-height: 1.45;
  }
  .ps-list {
    list-style: none;
    margin: 0.7rem 0 0;
    padding: 0.6rem 0 0;
    border-top: 1px dashed var(--line);
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  .ps-list li {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.7rem;
  }
  .ps-prop {
    font-size: 0.82rem;
    color: var(--text);
  }
  .ps-val {
    font-size: 0.82rem;
    color: var(--faint);
    font-variant-numeric: tabular-nums;
  }
  .back {
    display: inline-block;
    margin-bottom: 0.8rem;
    font-size: 0.8rem;
    color: var(--muted);
    text-decoration: none;
  }
  .back:hover {
    color: var(--ember);
  }
  .head {
    border-bottom: 1px solid var(--line);
    padding-bottom: 0.9rem;
    margin-bottom: 1rem;
  }
  .title-row {
    display: flex;
    align-items: baseline;
    gap: 0.6rem;
  }
  .name {
    margin: 0;
    font-size: 1.3rem;
    font-weight: 700;
    letter-spacing: -0.01em;
  }
  .dup-tag {
    font-size: 0.62rem;
    color: var(--muted);
    background: var(--panel-2);
    border: 1px solid var(--line);
    padding: 0.05rem 0.4rem;
    border-radius: 4px;
  }
  .rollup {
    margin-left: auto;
    font-size: 0.72rem;
    color: var(--muted);
    padding: 0.1rem 0.5rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--panel-2);
  }
  .rollup.ready {
    color: var(--good);
    border-color: var(--good);
  }
  .badges {
    display: flex;
    gap: 0.4rem;
    margin-top: 0.45rem;
  }
  .badge {
    font-size: 0.68rem;
    color: var(--muted);
    background: var(--panel-2);
    border: 1px solid var(--line);
    padding: 0.1rem 0.5rem;
    border-radius: 999px;
  }
  .badge.main {
    color: var(--ember);
    border-color: var(--ember-dim);
  }

  .actions {
    display: flex;
    gap: 0.5rem;
    margin-top: 0.9rem;
  }
  .own-toggle,
  .wish {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.4rem 0.8rem;
    background: var(--panel-2);
    border: 1px solid var(--line);
    border-radius: 8px;
    color: var(--muted);
    font-size: 0.82rem;
    cursor: pointer;
    transition: all 90ms;
  }
  .own-toggle:hover,
  .wish:hover {
    color: var(--text);
    border-color: var(--ember-dim);
  }
  .own-toggle .check {
    width: 1ch;
    color: var(--ember);
    font-weight: 700;
  }
  .own-toggle.on {
    background: var(--ember);
    border-color: var(--ember);
    color: var(--on-ember);
    font-weight: 600;
  }
  .own-toggle.on .check {
    color: var(--on-ember);
  }
  .wish.on {
    color: var(--ember);
    border-color: var(--ember-dim);
    background: var(--ember-glow);
  }
  .action-error {
    margin: 0.5rem 0 0;
    font-size: 0.8rem;
    color: var(--bad);
  }

  .presets {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    margin-bottom: 0.9rem;
  }
  .presets-label {
    font-size: 0.68rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--faint);
    margin-right: 0.2rem;
  }
  .preset {
    padding: 0.25rem 0.7rem;
    background: var(--panel-2);
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--muted);
    font-size: 0.76rem;
    cursor: pointer;
    transition: all 90ms;
  }
  .preset:hover {
    color: var(--ember);
    border-color: var(--ember-dim);
  }
  /* "Open at" default-quality picker — right-aligned in the preset row. */
  .default-pick {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.68rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--faint);
  }
  .default-pick select {
    text-transform: none;
    letter-spacing: 0;
    font-size: 0.76rem;
    color: var(--muted);
    background: var(--panel-2);
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 0.2rem 0.4rem;
    cursor: pointer;
    outline: none;
  }
  .default-pick select:focus {
    border-color: var(--ember);
  }

  .slots {
    display: flex;
    flex-direction: column;
    gap: 0.7rem;
  }

  .craft-time {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.7rem 1rem;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--panel);
  }
  .ct-label {
    font-size: 0.72rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
  }
  .ct-value {
    font-size: 0.95rem;
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }

  .missions {
    margin-top: 0;
  }
  .missions h3 {
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
    margin: 0 0 0.4rem;
  }
  .missions-link {
    display: inline-block;
    margin-bottom: 0.5rem;
    font-size: 0.78rem;
    color: var(--ember);
    text-decoration: none;
  }
  .missions-link:hover {
    text-decoration: underline;
  }
  .missions ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }
  .mission {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    color: var(--text);
    font-size: 0.86rem;
    padding: 0.1rem 0;
  }
  .once {
    font-size: 0.6rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--warn);
    border: 1px solid var(--line);
    border-radius: 4px;
    padding: 0.02rem 0.3rem;
  }

  .status {
    color: var(--muted);
    font-size: 0.88rem;
  }
</style>
