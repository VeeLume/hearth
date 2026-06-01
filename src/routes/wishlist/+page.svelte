<script lang="ts">
  import { onMount } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import {
    commands,
    type BpView,
    type MissionRef,
    type WishIntent,
  } from "$lib/bindings";
  import { categoryFor } from "$lib/categories";
  import { type Craftable, nameOf, collapseCraftables } from "$lib/catalog";

  // The wishlist is fulfilment-focused: the catalog is where you *find* and
  // flag things; here you see how to *get* them. Two intents, two questions:
  //   - Blueprints wanted (⚐) → which missions grant this BP?  (Stage 6)
  //   - Items wanted (♡)      → can I craft it, or can someone craft it for me?
  //                             (self-craft v1.5 · community v2)
  // The fulfilment rows below are placeholders until those surfaces exist.

  let blueprints = $state<BpView[]>([]);
  let owned = new SvelteSet<string>();
  let wishRecipe = new SvelteSet<string>();
  let wishItem = new SvelteSet<string>();
  // blueprint_record_guid → missions that grant it (the ⚐ fulfilment source).
  let grantedBy = $state<Partial<Record<string, MissionRef[]>>>({});
  let loading = $state(true);
  let errorMessage = $state<string | null>(null);

  onMount(async () => {
    const [bpResult, ownedResult, wishResult, missionResult] = await Promise.all([
      commands.listBlueprints(),
      commands.listOwned(),
      commands.listWishlist(),
      commands.missionsByBlueprint(),
    ]);
    if (bpResult.status === "ok") blueprints = bpResult.data;
    else errorMessage = `${bpResult.error.kind}: ${bpResult.error.message}`;
    if (ownedResult.status === "ok")
      for (const o of ownedResult.data) owned.add(o.blueprint_guid);
    if (wishResult.status === "ok")
      for (const w of wishResult.data)
        (w.intent === "recipe" ? wishRecipe : wishItem).add(w.blueprint_guid);
    if (missionResult.status === "ok") grantedBy = missionResult.data;
    loading = false;
  });

  /** Missions that grant any of a craftable's interchangeable BPs, deduped by
   *  mission and sorted by title — the ⚐ fulfilment answer. */
  function grantingMissions(c: Craftable): MissionRef[] {
    const seen = new Set<string>();
    const out: MissionRef[] = [];
    for (const g of c.bpGuids)
      for (const m of grantedBy[g] ?? []) {
        if (seen.has(m.mission_id)) continue;
        seen.add(m.mission_id);
        out.push(m);
      }
    out.sort((a, b) =>
      (a.title ?? a.mission_id).localeCompare(b.title ?? b.mission_id),
    );
    return out;
  }

  /** Compact label for the fulfilment chip. */
  function grantLabel(ms: MissionRef[]): string {
    if (ms.length === 1) return `granted by ${ms[0].title ?? ms[0].mission_id}`;
    return `granted by ${ms.length} missions`;
  }

  /** Deep-link to the Missions view, pre-filtered to the missions granting any
   *  of this craftable's interchangeable BPs (name labels the banner there). */
  function missionsLink(c: Craftable): string {
    const params = new URLSearchParams({
      bp: c.bpGuids.join(","),
      name: nameOf(c.rep),
    });
    return `/missions?${params}`;
  }

  function wishSet(intent: WishIntent): SvelteSet<string> {
    return intent === "recipe" ? wishRecipe : wishItem;
  }

  function craftableOwned(c: Craftable): boolean {
    return c.bpGuids.some((g) => owned.has(g));
  }

  /** Optimistic flip of one intent for one BP record. */
  async function toggleWant(guid: string, intent: WishIntent) {
    const set = wishSet(intent);
    const was = set.has(guid);
    if (was) set.delete(guid);
    else set.add(guid);
    const res = await commands.toggleWishlist(guid, intent);
    if (res.status === "ok") {
      if (res.data) set.add(guid);
      else set.delete(guid);
    } else {
      if (was) set.add(guid);
      else set.delete(guid);
      errorMessage = `${res.error.kind}: ${res.error.message}`;
    }
  }

  /** Remove a craftable from one wishlist intent (clears every BP). */
  async function removeWant(c: Craftable, intent: WishIntent) {
    for (const g of c.bpGuids.filter((g) => wishSet(intent).has(g)))
      await toggleWant(g, intent);
  }

  const craftables = $derived(collapseCraftables(blueprints));

  const wantedBp = $derived(
    craftables
      .filter((c) => c.bpGuids.some((g) => wishRecipe.has(g)))
      .sort((a, b) => nameOf(a.rep).localeCompare(nameOf(b.rep))),
  );
  const wantedItem = $derived(
    craftables
      .filter((c) => c.bpGuids.some((g) => wishItem.has(g)))
      .sort((a, b) => nameOf(a.rep).localeCompare(nameOf(b.rep))),
  );

  function categoryLabel(bp: BpView): string {
    const cat = categoryFor(bp.category_raw, bp.item_type, bp.item_sub_type);
    return cat.sub ? `${cat.main} · ${cat.sub}` : cat.main;
  }
</script>

<header class="topbar">
  <div class="page-title">
    <h1>Wishlist</h1>
    <span class="subtitle">
      {#if loading}Loading…{:else}{wantedBp.length} blueprint{wantedBp.length === 1 ? "" : "s"} · {wantedItem.length} item{wantedItem.length === 1 ? "" : "s"} wanted{/if}
    </span>
  </div>
</header>

{#if loading}
  <p class="status">Loading…</p>
{:else if errorMessage}
  <div class="error"><strong>Couldn't load the wishlist.</strong><p>{errorMessage}</p></div>
{:else}
  <section class="wl">
    <!-- ── Blueprints wanted (⚐) ─────────────────────────────────── -->
    <div class="wl-section">
      <div class="wl-head">
        <span class="wl-icon">⚐</span>
        <h2>Blueprints wanted</h2>
        <span class="wl-count">{wantedBp.length}</span>
      </div>
      <p class="wl-intro">
        Recipes you're hunting. Acquired by completing missions whose reward
        pools grant them.
      </p>
      {#if wantedBp.length === 0}
        <p class="wl-empty">
          Nothing here yet. Flag blueprints with <span class="ic">⚐</span> in the
          catalog to start a hunt list.
        </p>
      {:else}
        <ul>
          {#each wantedBp as c (c.entityKey)}
            {@const ms = grantingMissions(c)}
            <li class="wl-row">
              <span class="wl-name">{nameOf(c.rep)}</span>
              <span class="wl-cat">{categoryLabel(c.rep)}</span>
              {#if ms.length > 0}
                <a
                  class="fulfil granted"
                  href={missionsLink(c)}
                  title={`View the ${ms.length} mission${ms.length === 1 ? "" : "s"} that grant this blueprint`}
                >
                  {grantLabel(ms)} →
                </a>
              {:else}
                <span
                  class="fulfil none"
                  title="No mission in the current SC data grants this blueprint — it may be default-unlocked or acquired another way"
                >
                  no known mission source
                </span>
              {/if}
              <button
                class="wl-remove"
                title="Remove from wishlist"
                onclick={() => removeWant(c, "recipe")}
              >×</button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    <!-- ── Items wanted (♡) ──────────────────────────────────────── -->
    <div class="wl-section">
      <div class="wl-head">
        <span class="wl-icon">♡</span>
        <h2>Items wanted</h2>
        <span class="wl-count">{wantedItem.length}</span>
      </div>
      <p class="wl-intro">
        Crafted copies you want in hand. Make them yourself once you own the
        blueprint, or have a community member craft them for you.
      </p>
      {#if wantedItem.length === 0}
        <p class="wl-empty">
          Nothing here yet. Flag items with <span class="ic">♡</span> in the catalog.
        </p>
      {:else}
        <ul>
          {#each wantedItem as c (c.entityKey)}
            {@const isOwned = craftableOwned(c)}
            <li class="wl-row">
              <span class="wl-name">{nameOf(c.rep)}</span>
              <span class="wl-cat">{categoryLabel(c.rep)}</span>
              {#if isOwned}
                <span class="fulfil soon ready" title="You own the blueprint; the 'what can I craft' view arrives in v1.5">
                  ✓ own BP · craft it yourself · soon
                </span>
              {:else}
                <span class="fulfil soon" title="Acquire the blueprint, or get a community member to craft it (v2)">
                  needs BP · or community craft · soon
                </span>
              {/if}
              <button
                class="wl-remove"
                title="Remove from wishlist"
                onclick={() => removeWant(c, "item")}
              >×</button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </section>
{/if}

<style>
  .topbar {
    display: flex;
    align-items: center;
    gap: 1.5rem;
    padding: 1.1rem 1.6rem;
    border-bottom: 1px solid var(--line);
  }
  .page-title {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }
  h1 {
    margin: 0;
    font-size: 1.4rem;
    letter-spacing: -0.02em;
  }
  .subtitle {
    font-size: 0.78rem;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .status,
  .error {
    padding: 1rem 1.6rem;
    color: var(--muted);
  }
  .error strong {
    color: var(--bad);
  }

  .wl {
    flex: 1;
    overflow-y: auto;
    padding: 0.5rem 1.6rem 2rem;
  }
  .wl-section {
    margin-bottom: 2rem;
  }
  .wl-head {
    display: flex;
    align-items: baseline;
    gap: 0.6rem;
    padding: 0.6rem 0.2rem 0.2rem;
    border-bottom: 1px solid var(--line);
  }
  .wl-icon {
    font-size: 1rem;
    color: var(--ember);
  }
  .wl-head h2 {
    margin: 0;
    font-size: 1rem;
    font-weight: 700;
    letter-spacing: -0.01em;
    color: var(--ember);
  }
  .wl-count {
    font-size: 0.72rem;
    color: var(--faint);
    font-variant-numeric: tabular-nums;
  }
  .wl-intro {
    margin: 0.5rem 0.2rem 0.4rem;
    font-size: 0.8rem;
    color: var(--muted);
    max-width: 60ch;
  }
  .wl-empty {
    margin: 0.6rem 0.2rem;
    font-size: 0.85rem;
    color: var(--faint);
    font-style: italic;
  }
  .wl-empty .ic {
    font-style: normal;
    color: var(--muted);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .wl-row {
    display: flex;
    align-items: center;
    gap: 0.8rem;
    padding: 0.5rem 0.6rem;
    border-radius: 8px;
    border: 1px solid transparent;
  }
  .wl-row:hover {
    background: var(--panel);
  }
  .wl-name {
    flex: 0 1 auto;
    font-size: 0.9rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .wl-cat {
    flex: 0 0 auto;
    font-size: 0.7rem;
    color: var(--faint);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  /* Fulfilment hint — pushed to the right; muted until the real surface ships. */
  .fulfil {
    margin-left: auto;
    flex: 0 0 auto;
    font-size: 0.72rem;
    color: var(--muted);
    padding: 0.1rem 0.5rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--panel-2);
  }
  .fulfil.soon {
    opacity: 0.7;
  }
  .fulfil.ready {
    color: var(--ember);
    border-color: var(--ember-dim);
  }
  /* ⚐ fulfilment is live: a real mission source exists — links to the
     Missions view filtered to those missions. */
  .fulfil.granted {
    color: var(--ember);
    border-color: var(--ember-dim);
    text-decoration: none;
    cursor: pointer;
    transition: all 90ms;
  }
  .fulfil.granted:hover {
    background: var(--ember-glow);
  }
  /* No mission grants it (default-unlocked, or acquired some other way). */
  .fulfil.none {
    font-style: italic;
    color: var(--faint);
  }
  .wl-remove {
    flex: 0 0 auto;
    width: 1.5rem;
    height: 1.5rem;
    display: grid;
    place-items: center;
    border-radius: 6px;
    border: 1px solid var(--line);
    background: transparent;
    color: var(--faint);
    cursor: pointer;
    font-size: 1rem;
    line-height: 1;
    transition: all 90ms;
  }
  .wl-remove:hover {
    color: var(--bad);
    border-color: var(--bad);
  }
</style>
