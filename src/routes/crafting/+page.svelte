<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import { type Craftable, collapseCraftables, nameOf } from "$lib/domain/catalog";
  import { categoryFor } from "$lib/domain/categories";
  import { coverageFor } from "$lib/domain/inventory";
  import {
    data,
    owned,
    wishSet,
    ensureBlueprints,
    ensureOwnership,
    ensureGrantedBy,
    ensureInventory,
  } from "$lib/state/data.svelte";
  import PageHeader from "$lib/components/PageHeader.svelte";
  import Loading from "$lib/components/Loading.svelte";
  import RecipeDetail from "$lib/components/recipe/RecipeDetail.svelte";
  import { persistentScroll } from "$lib/actions/scroll";

  // The crafting planner: not a second catalogue, but "what can I do" — items you
  // want to make, recipes your materials can craft right now, and the ones you're
  // close on. Selecting any opens the shared recipe detail (URL `?bp=`).

  let loading = $state(!(data.blueprintsReady && data.ownershipReady));
  let errorMessage = $state<string | null>(null);
  // Preserve the planner scroll position across the sections ↔ detail toggle.
  const keepScroll = persistentScroll();

  onMount(async () => {
    const [bpErr] = await Promise.all([
      ensureBlueprints(),
      ensureOwnership(),
      ensureGrantedBy(),
      ensureInventory(),
    ]);
    if (bpErr) errorMessage = bpErr;
    loading = false;
  });

  const hasInventory = $derived(data.inventory.length > 0);
  const allCraftables = $derived(collapseCraftables(data.blueprints));
  const withRecipe = $derived(allCraftables.filter((c) => c.rep.recipe));

  function craftableOwned(c: Craftable) {
    return c.bpGuids.some((g) => owned.has(g));
  }
  function wantsItem(c: Craftable) {
    return c.bpGuids.some((g) => wishSet("item").has(g));
  }
  function cov(c: Craftable) {
    return coverageFor(c.rep.recipe, data.inventoryByCrc);
  }
  function satisfiedRatio(c: Craftable) {
    const x = cov(c);
    if (!x || x.ingredients.length === 0) return 0;
    return x.ingredients.filter((i) => i.satisfied).length / x.ingredients.length;
  }
  function covSummary(c: Craftable): { label: string; ready: boolean } | null {
    const x = cov(c);
    if (!x) return null;
    if (x.craftable) return { label: "✓ have materials", ready: true };
    const have = x.ingredients.filter((i) => i.satisfied).length;
    return { label: `materials ${have}/${x.ingredients.length}`, ready: false };
  }
  function categoryLabel(c: Craftable): string {
    const k = categoryFor(c.rep.category_raw, c.rep.item_type, c.rep.item_sub_type);
    return k.sub ? `${k.main} · ${k.sub}` : k.main;
  }

  const ready = $derived.by(() =>
    hasInventory
      ? withRecipe
          .filter((c) => craftableOwned(c) && cov(c)?.craftable)
          .sort((a, b) => nameOf(a.rep).localeCompare(nameOf(b.rep)))
      : [],
  );
  const almost = $derived.by(() =>
    hasInventory
      ? withRecipe
          .filter((c) => {
            if (!craftableOwned(c)) return false;
            const x = cov(c);
            return !!x && x.anyTracked && !x.craftable;
          })
          .sort(
            (a, b) =>
              satisfiedRatio(b) - satisfiedRatio(a) || nameOf(a.rep).localeCompare(nameOf(b.rep)),
          )
      : [],
  );
  const wantToMake = $derived(
    allCraftables.filter(wantsItem).sort((a, b) => nameOf(a.rep).localeCompare(nameOf(b.rep))),
  );

  // Selected recipe (the detail view) is URL-driven so back/forward work.
  const selectedBp = $derived(page.url.searchParams.get("bp"));
  const selectedCraftable = $derived(
    selectedBp
      ? (allCraftables.find(
          (c) => c.bpGuids.includes(selectedBp) || c.rep.blueprint_record_guid === selectedBp,
        ) ?? null)
      : null,
  );
</script>

<PageHeader title="Crafting">
  {#snippet subtitle()}
    {#if loading}Loading…{:else if selectedCraftable}{nameOf(selectedCraftable.rep)}{:else}plan what to craft{/if}
  {/snippet}
</PageHeader>

{#if loading}
  <Loading />
{:else if errorMessage}
  <div class="error"><strong>Couldn't load blueprints.</strong><p>{errorMessage}</p></div>
{:else if selectedCraftable}
  <div class="detail-wrap">
    <RecipeDetail craftable={selectedCraftable} backHref="/crafting" />
  </div>
{:else}
  <section class="planner" use:keepScroll>
    <!-- ── Want to make (♡) ─────────────────────────────────────── -->
    <div class="sec">
      <div class="sec-head"><span class="ic">♡</span><h2>Want to make</h2><span class="n">{wantToMake.length}</span></div>
      {#if wantToMake.length === 0}
        <p class="empty">Flag items with <span class="lit">♡</span> in the catalog to plan crafts.</p>
      {:else}
        <ul>
          {#each wantToMake as c (c.entityKey)}
            {@const isOwned = craftableOwned(c)}
            {@const cs = isOwned && hasInventory ? covSummary(c) : null}
            <li>
              <a class="row" href="?bp={c.rep.blueprint_record_guid}">
                <span class="row-name">{nameOf(c.rep)}</span>
                <span class="row-cat">{categoryLabel(c)}</span>
                {#if cs}
                  <span class="row-status" class:ready={cs.ready}>{cs.label}</span>
                {:else if isOwned}
                  <span class="row-status ready">✓ own BP</span>
                {:else}
                  <span class="row-status needbp">needs BP</span>
                {/if}
              </a>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    <!-- ── Ready to craft (✓) ───────────────────────────────────── -->
    <div class="sec">
      <div class="sec-head"><span class="ic">✓</span><h2>Ready to craft</h2><span class="n">{ready.length}</span></div>
      {#if !hasInventory}
        <p class="empty">
          Turn on <a href="/resources">resource sync</a> to see which of your owned recipes your
          materials can make right now.
        </p>
      {:else if ready.length === 0}
        <p class="empty">No owned recipe is fully covered by your current materials.</p>
      {:else}
        <ul>
          {#each ready as c (c.entityKey)}
            <li>
              <a class="row" href="?bp={c.rep.blueprint_record_guid}">
                <span class="row-name">{nameOf(c.rep)}</span>
                <span class="row-cat">{categoryLabel(c)}</span>
                <span class="row-status ready">✓ have materials</span>
              </a>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    <!-- ── Almost (◷) — only meaningful with a synced inventory ──── -->
    {#if hasInventory}
      <div class="sec">
        <div class="sec-head"><span class="ic">◷</span><h2>Almost — missing materials</h2><span class="n">{almost.length}</span></div>
        {#if almost.length === 0}
          <p class="empty">Nothing partially covered right now.</p>
        {:else}
          <ul>
            {#each almost as c (c.entityKey)}
              {@const cs = covSummary(c)}
              <li>
                <a class="row" href="?bp={c.rep.blueprint_record_guid}">
                  <span class="row-name">{nameOf(c.rep)}</span>
                  <span class="row-cat">{categoryLabel(c)}</span>
                  {#if cs}<span class="row-status">{cs.label}</span>{/if}
                </a>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    {/if}
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

  .planner {
    flex: 1;
    overflow-y: auto;
    padding: 0.5rem 1.6rem 2rem;
  }
  .sec {
    margin-bottom: 1.8rem;
  }
  .sec-head {
    display: flex;
    align-items: baseline;
    gap: 0.6rem;
    padding: 0.6rem 0.2rem 0.2rem;
    border-bottom: 1px solid var(--line);
  }
  .sec-head .ic {
    color: var(--ember);
    font-size: 1rem;
  }
  .sec-head h2 {
    margin: 0;
    font-size: 1rem;
    font-weight: 700;
    letter-spacing: -0.01em;
    color: var(--ember);
  }
  .sec-head .n {
    font-size: 0.72rem;
    color: var(--faint);
    font-variant-numeric: tabular-nums;
  }
  .empty {
    margin: 0.6rem 0.2rem;
    font-size: 0.85rem;
    color: var(--faint);
    font-style: italic;
  }
  .empty .lit {
    font-style: normal;
    color: var(--muted);
  }
  .empty a {
    color: var(--ember);
  }
  ul {
    list-style: none;
    margin: 0.3rem 0 0;
    padding: 0;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 0.8rem;
    padding: 0.5rem 0.6rem;
    border-radius: 8px;
    border: 1px solid transparent;
    color: inherit;
    text-decoration: none;
  }
  .row:hover {
    background: var(--panel);
    border-color: var(--line);
  }
  .row-name {
    flex: 0 1 auto;
    font-size: 0.9rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row-cat {
    flex: 0 0 auto;
    font-size: 0.7rem;
    color: var(--faint);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .row-status {
    margin-left: auto;
    flex: 0 0 auto;
    font-size: 0.72rem;
    color: var(--muted);
    padding: 0.1rem 0.5rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--panel-2);
  }
  .row-status.ready {
    color: var(--good);
    border-color: var(--good);
  }
  .row-status.needbp {
    color: var(--faint);
    font-style: italic;
  }
</style>
