<script lang="ts">
  import { onMount } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { commands, type BpView } from "$lib/bindings";
  import { categoryFor } from "$lib/categories";

  let blueprints = $state<BpView[]>([]);
  // SvelteSet is reactive on .has() / .add() / .delete() — using a
  // plain Set wrapped in $state had subtle reactivity gaps where
  // dependent template expressions ({@const isOwned = owned.has(...)})
  // didn't always re-run after toggle.
  let owned = new SvelteSet<string>();
  let loading = $state(true);
  let errorMessage = $state<string | null>(null);
  let query = $state("");
  // Expansion keys for rows currently showing their recipe / variant
  // panel. Leaf rows key on blueprint_record_guid; variant bundles
  // key on a stable composite (`bundle:<sorted-joined-guids>`).
  let expanded = new SvelteSet<string>();

  type Filter = "all" | "owned" | "unowned";
  let filter = $state<Filter>("all");

  function toggleExpanded(key: string) {
    if (expanded.has(key)) expanded.delete(key);
    else expanded.add(key);
  }

  // ─── Variant bundling ─────────────────────────────────────────────
  // CIG ships every armour skin / weapon paint as its own blueprint
  // crafting its own entity. The structural link between variants is
  // the **model tag** in the DCB tag tree: every Coda Pistol variant
  // (base + paints + "Modified") shares the leaf tag
  // `Weapon / FPS / Pistol / Coda`; every Geist Armor Arms variant
  // shares `Armor / FPS / Set / ClarkeDefense / FBL-8a`.
  //
  // The loader resolves that tag once per BP and ships it as
  // `bp.model_id`. We bundle BPs by (subgroup × model_id): same
  // model_id within a subgroup ⇒ one collapsible row. The fallback
  // when an entity has no recognised model tag is the crafted-entity
  // GUID, so BPs that share an entity (multiple recipes for the same
  // item, e.g. Cryo-Star SL) also bundle, while unique items stay as
  // singletons.
  //
  // Recipes are *not* part of the bundle key any more — earlier we
  // bundled by recipe equality and that produced cross-family bundles
  // (LH86 + S-38 + Salvo + Coda pistols all under one row because
  // they happen to share a 3-ingredient recipe). The recipe panel
  // still displays the shared recipe of the bundle's first member;
  // SC 4.8 data has every same-model variant on the same recipe.

  type Leaf = { kind: "leaf"; bp: BpView; sortName: string };
  type Bundle = {
    kind: "bundle";
    /** Shortest display name in the bundle — typically the un-skinned base. */
    baseName: string;
    /** Members sorted by name length (shortest first → base on top). */
    bps: BpView[];
    /** Sort key for the parent list (== baseName). */
    sortName: string;
    /** Stable expand-set key, independent of render order. */
    expandKey: string;
    /** Recipe shown in the expansion (first member's recipe — all
     *  members share it in SC 4.8 data). */
    recipe: BpView["recipe"];
  };
  type GroupItem = Leaf | Bundle;

  function bundleItems(items: BpView[]): GroupItem[] {
    const byModel = new Map<string, BpView[]>();
    const unkeyed: BpView[] = []; // BPs with no model_id — render as singletons
    for (const bp of items) {
      if (!bp.model_id) {
        unkeyed.push(bp);
        continue;
      }
      const arr = byModel.get(bp.model_id) ?? [];
      arr.push(bp);
      byModel.set(bp.model_id, arr);
    }

    const out: GroupItem[] = [];
    for (const arr of byModel.values()) {
      if (arr.length === 1) {
        const bp = arr[0];
        out.push({
          kind: "leaf",
          bp,
          sortName: bp.display_name ?? bp.blueprint_record_guid,
        });
      } else {
        const sorted = [...arr].sort((a, b) => {
          const la = (a.display_name ?? a.blueprint_record_guid).length;
          const lb = (b.display_name ?? b.blueprint_record_guid).length;
          return la - lb || (a.display_name ?? "").localeCompare(b.display_name ?? "");
        });
        const baseName = sorted[0].display_name ?? sorted[0].blueprint_record_guid;
        const expandKey =
          "bundle:" + sorted.map((b) => b.blueprint_record_guid).slice().sort().join(",");
        out.push({
          kind: "bundle",
          baseName,
          bps: sorted,
          sortName: baseName,
          expandKey,
          recipe: sorted[0].recipe,
        });
      }
    }
    for (const bp of unkeyed) {
      out.push({
        kind: "leaf",
        bp,
        sortName: bp.display_name ?? bp.blueprint_record_guid,
      });
    }
    out.sort((a, b) => a.sortName.localeCompare(b.sortName));
    return out;
  }

  /** Strip the base prefix from a variant's display name. Returns
   *  "Standard" for the variant whose name IS the base. */
  function variantSuffix(fullName: string, baseName: string): string {
    if (fullName === baseName) return "Standard";
    if (fullName.startsWith(baseName)) {
      return fullName.slice(baseName.length).trim() || "Standard";
    }
    return fullName;
  }

  /** Format a craft time in seconds as a short human string. */
  function formatCraftTime(seconds: number | null): string {
    if (seconds == null || seconds <= 0) return "—";
    const h = Math.floor(seconds / 3600);
    const m = Math.floor((seconds % 3600) / 60);
    const s = Math.round(seconds % 60);
    if (h > 0) return m > 0 ? `${h}h ${m}m` : `${h}h`;
    if (m > 0) return s > 0 ? `${m}m ${s}s` : `${m}m`;
    return `${s}s`;
  }

  /** Format an SCU quantity. Most recipe ingredients are << 1 SCU
   *  (e.g. 0.02), so default to 2 decimals; widen for larger values. */
  function formatScu(scu: number | null): string {
    if (scu == null) return "?";
    if (scu < 1) return scu.toFixed(2);
    if (scu < 10) return scu.toFixed(1);
    return scu.toFixed(0);
  }

  onMount(async () => {
    const [bpResult, ownedResult] = await Promise.all([
      commands.listBlueprints(),
      commands.listOwned(),
    ]);
    if (bpResult.status === "ok") {
      blueprints = bpResult.data;
    } else {
      errorMessage = `${bpResult.error.kind}: ${bpResult.error.message}`;
    }
    if (ownedResult.status === "ok") {
      owned.clear();
      for (const o of ownedResult.data) owned.add(o.blueprint_guid);
    }
    loading = false;
  });

  async function toggleOwned(guid: string) {
    // Optimistic flip; reconcile from the command's returned truth.
    const wasOwned = owned.has(guid);
    if (wasOwned) owned.delete(guid);
    else owned.add(guid);

    const result = await commands.toggleOwned(guid);
    if (result.status === "ok") {
      // Server truth — only adjust if it disagrees with our optimistic flip.
      if (result.data) owned.add(guid);
      else owned.delete(guid);
    } else {
      // Revert.
      if (wasOwned) owned.add(guid);
      else owned.delete(guid);
      errorMessage = `${result.error.kind}: ${result.error.message}`;
    }
  }

  const ownedCount = $derived(
    blueprints.filter((b) => owned.has(b.blueprint_record_guid)).length,
  );

  const filtered = $derived.by(() => {
    const q = query.toLowerCase().trim();
    return blueprints.filter((bp) => {
      const isOwned = owned.has(bp.blueprint_record_guid);
      if (filter === "owned" && !isOwned) return false;
      if (filter === "unowned" && isOwned) return false;
      if (q) {
        const name = bp.display_name?.toLowerCase() ?? "";
        const guid = bp.blueprint_record_guid.toLowerCase();
        const cat = categoryFor(bp.category_raw, bp.item_type, bp.item_sub_type);
        const catText = `${cat.main} ${cat.sub}`.toLowerCase();
        if (!(name.includes(q) || guid.includes(q) || catText.includes(q)))
          return false;
      }
      return true;
    });
  });

  // Two-level grouping by sc-crafting Category (primary axis) and the
  // AttachDef item_type / sub_type (secondary axis). Within each
  // subgroup, BPs are bundled by recipe signature so skin variants
  // (same recipe, different name) collapse to one row. See
  // `bundleItems` above.
  type SubGroup = { sub: string; subOrder: number; items: GroupItem[]; rawCount: number };
  type MainGroup = {
    main: string;
    mainOrder: number;
    /** Raw blueprint count across all subgroups (NOT row count after
     *  bundling — that's the user-visible row total, computed in the
     *  template via subs.reduce). */
    total: number;
    subs: SubGroup[];
  };

  const grouped = $derived.by((): MainGroup[] => {
    // Pass 1: accumulate raw BpViews per main/sub bucket.
    type Pre = MainGroup & {
      subMap: Map<string, { sub: string; subOrder: number; raw: BpView[] }>;
    };
    const mains = new Map<string, Pre>();
    for (const bp of filtered) {
      const cat = categoryFor(bp.category_raw, bp.item_type, bp.item_sub_type);
      let main = mains.get(cat.main);
      if (!main) {
        main = {
          main: cat.main,
          mainOrder: cat.mainOrder,
          total: 0,
          subs: [],
          subMap: new Map(),
        };
        mains.set(cat.main, main);
      }
      let sub = main.subMap.get(cat.sub);
      if (!sub) {
        sub = { sub: cat.sub, subOrder: cat.subOrder, raw: [] };
        main.subMap.set(cat.sub, sub);
      }
      sub.raw.push(bp);
      main.total += 1;
    }

    // Pass 2: bundle each subgroup, build final SubGroup shape.
    const out: MainGroup[] = [];
    for (const m of mains.values()) {
      const subs: SubGroup[] = [];
      for (const s of m.subMap.values()) {
        subs.push({
          sub: s.sub,
          subOrder: s.subOrder,
          items: bundleItems(s.raw),
          rawCount: s.raw.length,
        });
      }
      subs.sort((a, b) => a.subOrder - b.subOrder || a.sub.localeCompare(b.sub));
      out.push({ main: m.main, mainOrder: m.mainOrder, total: m.total, subs });
    }
    out.sort((a, b) => a.mainOrder - b.mainOrder || a.main.localeCompare(b.main));
    return out;
  });

  /** Owned-variant count for a bundle, reactive on `owned`. */
  function bundleOwnedCount(b: Bundle): number {
    let n = 0;
    for (const bp of b.bps) if (owned.has(bp.blueprint_record_guid)) n++;
    return n;
  }

  const filters: { id: Filter; label: string }[] = [
    { id: "all", label: "All" },
    { id: "owned", label: "Owned" },
    { id: "unowned", label: "Unowned" },
  ];
</script>

<header class="topbar">
  <div class="page-title">
    <h1>Catalog</h1>
    <span class="subtitle">
      {#if loading}Loading…{:else}{blueprints.length} blueprints · {ownedCount} owned{/if}
    </span>
  </div>
  <input
    class="search"
    type="search"
    placeholder="Search name, type, or GUID…"
    bind:value={query}
    disabled={loading}
  />
</header>

{#if loading}
  <p class="status">
    Loading SC reference data… (first run after install / SC patch parses
    the Datacore — ~30 s; subsequent launches load the cached catalog in
    under a second)
  </p>
{:else if errorMessage}
  <div class="error">
    <strong>Couldn't load blueprints.</strong>
    <p>{errorMessage}</p>
    <p class="hint">
      Most common cause: Star Citizen isn't installed (or the RSI launcher
      has never run on this machine). Install/launch SC once and restart.
    </p>
  </div>
{:else}
  <div class="filterbar">
    <div class="chips">
      {#each filters as f (f.id)}
        <button class="chip" class:on={filter === f.id} onclick={() => (filter = f.id)}>
          {f.label}
          {#if f.id === "owned"}<span class="chip-n">{ownedCount}</span>{/if}
        </button>
      {/each}
    </div>
    <div class="legend">
      <span class="legend-item"><span class="legend-icon own">✓</span> own BP</span>
      <span class="legend-item soon-legend" title="Wishlist arrives in a later version">
        <span class="legend-icon">⚐</span> / <span class="legend-icon">♡</span> wishlist · soon
      </span>
    </div>
  </div>

  <section class="catalog">
    {#each grouped as mainGroup (mainGroup.main)}
      <div class="maincat">
        <div class="maincat-head">
          <h2>{mainGroup.main}</h2>
          <span class="maincat-count">{mainGroup.total}</span>
        </div>
        {#each mainGroup.subs as subGroup (subGroup.sub)}
          <div class="pool">
            {#if subGroup.sub}
              <div class="pool-head">
                <span class="pool-name">{subGroup.sub}</span>
                <span class="pool-count">{subGroup.items.length}</span>
              </div>
            {/if}
            <ul>
              {#each subGroup.items as item, i (item.kind === "leaf" ? item.bp.blueprint_record_guid : item.expandKey)}
                {#if item.kind === "leaf"}
                  {@const bp = item.bp}
                  {@const isOwned = owned.has(bp.blueprint_record_guid)}
                  {@const isExpanded = expanded.has(bp.blueprint_record_guid)}
                  <li class:owned={isOwned} class:expanded={isExpanded}>
                    <div class="bp-row">
                      <button
                        class="own-toggle"
                        class:on={isOwned}
                        title={isOwned ? "Blueprint owned — click to unmark" : "Mark blueprint owned"}
                        onclick={() => toggleOwned(bp.blueprint_record_guid)}
                      >
                        {isOwned ? "✓" : ""}
                      </button>
                      <button
                        class="bp-expand"
                        title={isExpanded ? "Hide recipe" : "Show recipe"}
                        onclick={() => toggleExpanded(bp.blueprint_record_guid)}
                      >
                        <span class="chevron" class:open={isExpanded} aria-hidden="true">▸</span>
                        <span class="bp-name">{bp.display_name ?? bp.blueprint_record_guid}</span>
                        {#if bp.display_name}
                          <span class="bp-guid">{bp.blueprint_record_guid}</span>
                        {/if}
                        {#if bp.recipe?.craft_time_seconds}
                          <span class="bp-time" title="Craft time">⏱ {formatCraftTime(bp.recipe.craft_time_seconds)}</span>
                        {/if}
                      </button>

                      <!-- Wishlist intents — present but disabled until Stage 7. -->
                      <div class="wish-group">
                        {#if !isOwned}
                          <span class="wish soon" title="Want blueprint — coming in a later version">⚐</span>
                        {:else}
                          <span class="wish placeholder-slot">·</span>
                        {/if}
                        <span class="wish soon" title="Want crafted item — coming in a later version">♡</span>
                      </div>
                    </div>

                    {#if isExpanded}
                      <div class="recipe-panel">
                        {#if bp.recipe}
                          {#if bp.recipe.ingredients.length > 0}
                            <ul class="ingredients">
                              {#each bp.recipe.ingredients as ing (ing.resource_guid)}
                                <li class="ingredient">
                                  <span class="ing-qty">{formatScu(ing.quantity_scu)} <span class="ing-unit">SCU</span></span>
                                  <span class="ing-name">{ing.resource_name ?? ing.resource_guid}</span>
                                  {#if ing.min_quality > 0}
                                    <span class="ing-quality" title="Minimum required quality">≥ Q{ing.min_quality}</span>
                                  {/if}
                                </li>
                              {/each}
                            </ul>
                          {:else}
                            <span class="recipe-empty">Recipe has no listed ingredients.</span>
                          {/if}
                        {:else}
                          <span class="recipe-empty">No recipe data for this blueprint.</span>
                        {/if}
                      </div>
                    {/if}
                  </li>
                {:else}
                  {@const bundle = item}
                  {@const isExpanded = expanded.has(bundle.expandKey)}
                  {@const ownedN = bundleOwnedCount(bundle)}
                  {@const totalN = bundle.bps.length}
                  {@const allOwned = ownedN === totalN}
                  {@const someOwned = ownedN > 0}
                  <li class:expanded={isExpanded} class:bundle-owned={allOwned} class:bundle-some={someOwned && !allOwned}>
                    <div class="bp-row bundle-row">
                      <!-- Variant count badge replaces the per-row checkmark.
                           Ownership is per-variant; expand to manage. -->
                      <span
                        class="variant-count"
                        class:full={allOwned}
                        class:some={someOwned && !allOwned}
                        title={`${ownedN} of ${totalN} variants owned`}
                      >
                        {ownedN}/{totalN}
                      </span>
                      <button
                        class="bp-expand"
                        title={isExpanded ? "Hide variants & recipe" : "Show variants & recipe"}
                        onclick={() => toggleExpanded(bundle.expandKey)}
                      >
                        <span class="chevron" class:open={isExpanded} aria-hidden="true">▸</span>
                        <span class="bp-name">{bundle.baseName}</span>
                        <span class="bundle-tag" title="Same recipe across these variants">variants</span>
                        {#if bundle.recipe?.craft_time_seconds}
                          <span class="bp-time" title="Craft time (shared)">⏱ {formatCraftTime(bundle.recipe.craft_time_seconds)}</span>
                        {/if}
                      </button>
                      <!-- Wishlist controls live per-variant inside the
                           expansion; the parent row has no wishlist
                           affordance. -->
                      <div class="wish-group"></div>
                    </div>

                    {#if isExpanded}
                      <div class="recipe-panel">
                        {#if bundle.recipe && bundle.recipe.ingredients.length > 0}
                          <ul class="ingredients">
                            {#each bundle.recipe.ingredients as ing (ing.resource_guid)}
                              <li class="ingredient">
                                <span class="ing-qty">{formatScu(ing.quantity_scu)} <span class="ing-unit">SCU</span></span>
                                <span class="ing-name">{ing.resource_name ?? ing.resource_guid}</span>
                                {#if ing.min_quality > 0}
                                  <span class="ing-quality" title="Minimum required quality">≥ Q{ing.min_quality}</span>
                                {/if}
                              </li>
                            {/each}
                          </ul>
                        {/if}
                        <div class="variant-list">
                          {#each bundle.bps as vbp (vbp.blueprint_record_guid)}
                            {@const vOwned = owned.has(vbp.blueprint_record_guid)}
                            {@const fullName = vbp.display_name ?? vbp.blueprint_record_guid}
                            {@const suffix = variantSuffix(fullName, bundle.baseName)}
                            {@const isBase = suffix === "Standard"}
                            <div class="variant" class:owned={vOwned}>
                              <button
                                class="own-toggle"
                                class:on={vOwned}
                                title={vOwned ? "Blueprint owned — click to unmark" : "Mark blueprint owned"}
                                onclick={() => toggleOwned(vbp.blueprint_record_guid)}
                              >
                                {vOwned ? "✓" : ""}
                              </button>
                              <span class="variant-name" class:base={isBase}>{suffix}</span>
                              <span class="bp-guid">{vbp.blueprint_record_guid}</span>
                              <div class="wish-group">
                                {#if !vOwned}
                                  <span class="wish soon" title="Want blueprint — coming in a later version">⚐</span>
                                {:else}
                                  <span class="wish placeholder-slot">·</span>
                                {/if}
                                <span class="wish soon" title="Want crafted item — coming in a later version">♡</span>
                              </div>
                            </div>
                          {/each}
                        </div>
                      </div>
                    {/if}
                  </li>
                {/if}
              {/each}
            </ul>
          </div>
        {/each}
      </div>
    {/each}
    {#if grouped.length === 0}
      <p class="status">No blueprints match.</p>
    {/if}
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
  .search {
    margin-left: auto;
    width: 280px;
    padding: 0.5rem 0.8rem;
    background: var(--panel-2);
    border: 1px solid var(--line);
    border-radius: 8px;
    outline: none;
    transition: border-color 90ms;
  }
  .search:focus {
    border-color: var(--ember);
  }
  .search:disabled {
    opacity: 0.6;
  }

  .status,
  .error {
    padding: 1rem 1.6rem;
    color: var(--muted);
  }
  .error strong {
    color: var(--bad);
  }
  .error .hint {
    color: var(--faint);
    font-size: 0.85rem;
    margin: 0.5rem 0 0;
  }

  .filterbar {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.85rem 1.6rem;
  }
  .chips {
    display: flex;
    gap: 0.5rem;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.32rem 0.75rem;
    background: var(--panel-2);
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--muted);
    font-size: 0.8rem;
    cursor: pointer;
    transition: all 90ms;
  }
  .chip:hover {
    color: var(--text);
  }
  .chip.on {
    background: var(--ember-glow);
    border-color: var(--ember-dim);
    color: var(--ember);
  }
  .chip-n {
    font-size: 0.68rem;
    opacity: 0.8;
    font-variant-numeric: tabular-nums;
  }
  .legend {
    margin-left: auto;
    display: flex;
    gap: 0.9rem;
    font-size: 0.72rem;
    color: var(--faint);
  }
  .legend-item {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
  }
  .legend-icon {
    font-size: 0.85rem;
  }
  .legend-icon.own {
    color: var(--ember);
  }
  .soon-legend {
    cursor: help;
  }

  .catalog {
    flex: 1;
    overflow-y: auto;
    padding: 0 1.6rem 2rem;
  }
  .maincat {
    margin-bottom: 1.75rem;
  }
  .maincat-head {
    display: flex;
    align-items: baseline;
    gap: 0.6rem;
    padding: 0.6rem 0.2rem 0.2rem;
    position: sticky;
    top: 0;
    z-index: 2;
    background: var(--bg);
    border-bottom: 1px solid var(--line);
  }
  .maincat-head h2 {
    margin: 0;
    font-size: 1rem;
    font-weight: 700;
    letter-spacing: -0.01em;
    color: var(--ember);
  }
  .maincat-count {
    font-size: 0.72rem;
    color: var(--faint);
    font-variant-numeric: tabular-nums;
  }
  .pool {
    margin-bottom: 0.5rem;
    margin-left: 0.25rem;
  }
  .pool-head {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0.4rem 0.2rem;
    position: sticky;
    /* stick below the main-category header (which is ~2.1rem tall) */
    top: 2.1rem;
    z-index: 1;
    background: var(--bg);
  }
  .pool-name {
    font-size: 0.78rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
  }
  .pool-count {
    font-size: 0.72rem;
    color: var(--faint);
    font-variant-numeric: tabular-nums;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    border-radius: 8px;
    border: 1px solid transparent;
  }
  li:hover {
    background: var(--panel);
  }
  li.owned {
    background: linear-gradient(90deg, var(--ember-glow), transparent 60%);
  }
  li.expanded {
    background: var(--panel);
    border-color: var(--line);
  }
  li.owned.expanded {
    background: linear-gradient(90deg, var(--ember-glow), var(--panel) 60%);
    border-color: var(--ember-dim);
  }
  .bp-row {
    display: flex;
    align-items: center;
    gap: 0.8rem;
    padding: 0.5rem 0.6rem;
  }
  .own-toggle {
    width: 1.4rem;
    height: 1.4rem;
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    border-radius: 6px;
    border: 1.5px solid var(--line);
    background: transparent;
    color: var(--bg);
    cursor: pointer;
    font-size: 0.8rem;
    transition: all 90ms;
  }
  .own-toggle:hover {
    border-color: var(--ember-dim);
  }
  .own-toggle.on {
    background: var(--ember);
    border-color: var(--ember);
    color: #1a1209;
    font-weight: 700;
  }
  /* The expand button is a transparent affordance over most of the row;
     it carries the name, chevron, GUID, and a craft-time chip. */
  .bp-expand {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 0.55rem;
    padding: 0.1rem 0.1rem;
    background: transparent;
    border: none;
    color: inherit;
    text-align: left;
    cursor: pointer;
    min-width: 0;
  }
  .bp-expand:hover .chevron,
  .bp-expand:focus-visible .chevron {
    color: var(--ember);
  }
  .chevron {
    width: 0.9rem;
    flex: 0 0 auto;
    color: var(--faint);
    font-size: 0.75rem;
    transition: transform 120ms ease-out, color 90ms;
    display: inline-block;
  }
  .chevron.open {
    transform: rotate(90deg);
    color: var(--ember);
  }
  .bp-name {
    flex: 1;
    font-size: 0.9rem;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bp-guid {
    font-family: ui-monospace, "SF Mono", Consolas, monospace;
    font-size: 0.72rem;
    color: var(--faint);
    flex: 0 0 auto;
  }
  .bp-time {
    flex: 0 0 auto;
    font-size: 0.72rem;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    padding: 0.1rem 0.45rem;
    border-radius: 999px;
    background: var(--panel-2);
  }
  .wish-group {
    display: flex;
    align-items: center;
    gap: 0.1rem;
  }
  .wish {
    font-size: 1.05rem;
    line-height: 1;
    padding: 0.25rem 0.3rem;
    color: var(--faint);
  }
  .wish.soon {
    opacity: 0.35;
    cursor: not-allowed;
  }
  .wish.placeholder-slot {
    opacity: 0.3;
  }

  /* ── Recipe panel (visible when a row is expanded) ── */
  .recipe-panel {
    padding: 0.4rem 0.8rem 0.7rem 2.4rem;
    border-top: 1px dashed var(--line);
    margin: 0 0.6rem;
  }
  .ingredients {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }
  .ingredient {
    display: flex;
    align-items: baseline;
    gap: 0.7rem;
    padding: 0.15rem 0;
    border: none;
  }
  .ingredient:hover {
    background: transparent;
  }
  .ing-qty {
    flex: 0 0 4.5rem;
    text-align: right;
    font-variant-numeric: tabular-nums;
    font-size: 0.82rem;
    color: var(--ember);
    font-weight: 500;
  }
  .ing-unit {
    font-size: 0.65rem;
    color: var(--faint);
    font-weight: 400;
    margin-left: 0.1rem;
  }
  .ing-name {
    flex: 1;
    font-size: 0.85rem;
    color: var(--text);
  }
  .ing-quality {
    font-size: 0.7rem;
    color: var(--muted);
    padding: 0.05rem 0.35rem;
    border: 1px solid var(--line);
    border-radius: 4px;
  }
  .recipe-empty {
    font-size: 0.8rem;
    color: var(--faint);
    font-style: italic;
  }

  /* ── Variant bundles ── */
  /* Parent row of a bundle (the "Geist Armor Arms · 4/7" header).
     No own-toggle button — ownership is per-variant, behind expansion. */
  .bundle-row .variant-count {
    width: 2.4rem;
    flex: 0 0 auto;
    text-align: center;
    font-size: 0.72rem;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
    background: var(--panel-2);
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 0.15rem 0.2rem;
    font-weight: 500;
  }
  .bundle-row .variant-count.some {
    color: var(--ember);
    border-color: var(--ember-dim);
  }
  .bundle-row .variant-count.full {
    color: #1a1209;
    background: var(--ember);
    border-color: var(--ember);
    font-weight: 700;
  }
  /* "variants" pill on the parent row label. */
  .bundle-tag {
    flex: 0 0 auto;
    font-size: 0.62rem;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--faint);
    padding: 0.05rem 0.4rem;
    border: 1px solid var(--line);
    border-radius: 4px;
  }
  /* Tint the row when ownership is partial/complete across the bundle. */
  li.bundle-some {
    background: linear-gradient(90deg, var(--ember-glow), transparent 60%);
    opacity: 0.97;
  }
  li.bundle-owned {
    background: linear-gradient(90deg, var(--ember-glow), transparent 40%);
  }

  /* Per-variant rows inside an expanded bundle. */
  .variant-list {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    margin-top: 0.45rem;
    padding-top: 0.45rem;
    border-top: 1px dashed var(--line);
  }
  .variant {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0.25rem 0;
    border-radius: 6px;
  }
  .variant:hover {
    background: var(--panel-2);
  }
  .variant.owned {
    background: linear-gradient(90deg, var(--ember-glow), transparent 50%);
  }
  .variant .own-toggle {
    width: 1.2rem;
    height: 1.2rem;
    font-size: 0.7rem;
  }
  .variant-name {
    flex: 1;
    font-size: 0.85rem;
    color: var(--text);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .variant-name.base {
    color: var(--muted);
    font-style: italic;
  }
  .variant .bp-guid {
    flex: 0 0 auto;
  }
  .variant .wish {
    font-size: 0.95rem;
    padding: 0.15rem 0.25rem;
  }
</style>
