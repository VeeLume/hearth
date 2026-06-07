<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import { type BpView, type MissionRef, type WishIntent } from "$lib/ipc";
  import { categoryFor } from "$lib/domain/categories";
  import Loading from "$lib/components/Loading.svelte";
  import PageHeader from "$lib/components/PageHeader.svelte";
  import RecipeDetail from "$lib/components/recipe/RecipeDetail.svelte";
  import { persistentScroll } from "$lib/actions/scroll";
  import { type Craftable, nameOf, collapseCraftables, missionsLink } from "$lib/domain/catalog";
  import {
    data,
    owned,
    wishRecipe,
    wishItem,
    wishSet,
    ensureBlueprints,
    ensureOwnership,
    ensureGrantedBy,
    toggleWishlist,
  } from "$lib/state/data.svelte";

  // A short overview of what you've flagged. Two intents, two questions:
  //   - Blueprints wanted (⚐) → which missions grant this BP?
  //   - Items wanted (♡)      → own the BP (craft it) or acquire it.
  // The recipe, materials coverage, and quality calculator live in the shared
  // recipe detail (open a row); this page stays a lean list.
  const blueprints = $derived(data.blueprints);
  const grantedBy = $derived(data.grantedBy);
  let loading = $state(!(data.blueprintsReady && data.ownershipReady && data.grantedByReady));
  let errorMessage = $state<string | null>(null);
  // Preserve the wishlist scroll position across the list ↔ detail toggle.
  const keepScroll = persistentScroll();

  onMount(async () => {
    const [bpErr] = await Promise.all([ensureBlueprints(), ensureOwnership(), ensureGrantedBy()]);
    if (bpErr) errorMessage = bpErr;
    loading = false;
  });

  // URL-driven recipe detail (`?bp=`), so back returns here.
  const selectedBp = $derived(page.url.searchParams.get("bp"));
  const selectedCraftable = $derived(
    selectedBp
      ? (collapseCraftables(blueprints).find(
          (c) => c.bpGuids.includes(selectedBp) || c.rep.blueprint_record_guid === selectedBp,
        ) ?? null)
      : null,
  );

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
    out.sort((a, b) => (a.title ?? a.mission_id).localeCompare(b.title ?? b.mission_id));
    return out;
  }
  function grantLabel(ms: MissionRef[]): string {
    if (ms.length === 1) return `granted by ${ms[0].title ?? ms[0].mission_id}`;
    return `granted by ${ms.length} missions`;
  }

  function craftableOwned(c: Craftable): boolean {
    return c.bpGuids.some((g) => owned.has(g));
  }

  /** Remove a craftable from one wishlist intent (clears every BP). */
  async function removeWant(c: Craftable, intent: WishIntent) {
    for (const g of c.bpGuids.filter((g) => wishSet(intent).has(g))) {
      const err = await toggleWishlist(g, intent);
      if (err) errorMessage = err;
    }
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

<PageHeader title="Wishlist">
  {#snippet subtitle()}
    {#if loading}Loading…{:else if selectedCraftable}{nameOf(selectedCraftable.rep)}{:else}{wantedBp.length} blueprint{wantedBp.length === 1 ? "" : "s"} · {wantedItem.length} item{wantedItem.length === 1 ? "" : "s"} wanted{/if}
  {/snippet}
</PageHeader>

{#if loading}
  <Loading />
{:else if errorMessage}
  <div class="error"><strong>Couldn't load the wishlist.</strong><p>{errorMessage}</p></div>
{:else if selectedCraftable}
  <div class="detail-wrap">
    <RecipeDetail craftable={selectedCraftable} backHref="/wishlist" />
  </div>
{:else}
  <section class="wl" use:keepScroll>
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
              <a class="wl-name" href="?bp={c.rep.blueprint_record_guid}">{nameOf(c.rep)}</a>
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
                <span class="fulfil none" title="No mission in the current SC data grants this blueprint — it may be default-unlocked or acquired another way">
                  no known mission source
                </span>
              {/if}
              <button class="wl-remove" title="Remove from wishlist" onclick={() => removeWant(c, "recipe")}>×</button>
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
        Crafted copies you want in hand. Open a row for the recipe, materials
        coverage, and quality. Make them once you own the blueprint, or have a
        community member craft them (v2).
      </p>
      {#if wantedItem.length === 0}
        <p class="wl-empty">
          Nothing here yet. Flag items with <span class="ic">♡</span> in the catalog.
        </p>
      {:else}
        <ul>
          {#each wantedItem as c (c.entityKey)}
            <li class="wl-row">
              <a class="wl-name" href="?bp={c.rep.blueprint_record_guid}">{nameOf(c.rep)}</a>
              <span class="wl-cat">{categoryLabel(c.rep)}</span>
              {#if craftableOwned(c)}
                <span class="fulfil ready" title="You own the blueprint — craft it in-game">✓ you can craft this</span>
              {:else}
                <span class="fulfil soon" title="Acquire the blueprint, or get a community member to craft it (v2)">needs BP · or community craft · soon</span>
              {/if}
              <button class="wl-remove" title="Remove from wishlist" onclick={() => removeWant(c, "item")}>×</button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </section>
{/if}

<style>
  .error {
    padding: 1rem 1.6rem;
    color: var(--muted);
  }
  .error strong {
    color: var(--bad);
  }

  .detail-wrap {
    flex: 1;
    overflow-y: auto;
    padding: 1.2rem 1.6rem 2rem;
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
    color: var(--text);
    text-decoration: none;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .wl-name:hover {
    color: var(--ember);
  }
  .wl-cat {
    flex: 0 0 auto;
    font-size: 0.7rem;
    color: var(--faint);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  /* Fulfilment chip — pushed to the right. */
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
  /* ⚐ fulfilment is live: links to the Missions view filtered to the granters. */
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
