<script lang="ts">
  import { onMount } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { commands, errText, type BpView, type WishIntent } from "$lib/ipc";
  import { categoryFor } from "$lib/categories";
  import Loading from "$lib/components/Loading.svelte";
  import {
    type Craftable,
    nameOf,
    variantSuffix,
    collapseCraftables,
    formatCraftTime,
    formatScu,
  } from "$lib/catalog";
  import {
    data,
    owned,
    wishRecipe,
    wishItem,
    wishSet,
    ensureBlueprints,
    ensureOwnership,
  } from "$lib/data.svelte";

  // Blueprints + ownership live in the shared store ($lib/data.svelte) so they
  // survive navigation (no per-page refetch / loading flash) and stay
  // consistent across pages. `blueprints` aliases the store for the template +
  // derived code below; `owned` / `wishRecipe` / `wishItem` / `wishSet` are the
  // shared reactive sets, imported above.
  const blueprints = $derived(data.blueprints);
  let loading = $state(!(data.blueprintsReady && data.ownershipReady));
  let errorMessage = $state<string | null>(null);
  // Live-sync button state (the loading screen handles its own path hint).
  let liveSync = $state<{ available: boolean; enabled: boolean } | null>(null);
  let syncing = $state(false);
  let query = $state("");
  // Expansion keys for rows currently showing their recipe / variant
  // panel. Leaf rows key on blueprint_record_guid; variant bundles
  // key on a stable composite (`bundle:<sorted-joined-guids>`).
  let expanded = new SvelteSet<string>();

  type Filter = "all" | "owned" | "unowned" | "wantbp" | "wantitem";
  let filter = $state<Filter>("all");

  /** Catalog-side live sync: fetch from the account, then reflect the
   *  reconciled ownership locally. Result/errors surface via notifications. */
  async function syncNow() {
    if (syncing) return;
    syncing = true;
    // Result + errors surface via notifications; the backend's ownership-changed
    // event refreshes the owned set (so all sync paths behave the same).
    await commands.liveSyncNow();
    syncing = false;
  }

  function toggleExpanded(key: string) {
    if (expanded.has(key)) expanded.delete(key);
    else expanded.add(key);
  }

  // ─── BP collapse & variant bundling ───────────────────────────────
  // CIG ships every armour skin / weapon paint as its own blueprint
  // crafting its own entity. sc-items groups those into a Model (one
  // design + one slot) and ships the model id on every BP as
  // `bp.family_id`, plus the design's base-item name as
  // `bp.family_base_name`. The fallback when a BP has no model
  // (non-gear: ship components, props) is the crafted-entity GUID, so
  // those stay as singletons.
  //
  // Two collapses happen here, in order:
  //   1. Same-entity BPs → one Craftable. Several blueprints can craft
  //      the exact same item (same crafted_entity_guid, same recipe in
  //      SC 4.8). Owning any one lets you craft it, so we fold them:
  //      ownership is "own ANY of these BPs".
  //   2. Same-model Craftables → one Bundle (the collapsible variant
  //      row), titled by the base item. A model whose only blueprint is
  //      a colorway (no base BP) still renders as a bundle — same shape,
  //      just a 0/1 count and no "Standard" row in the expansion. Only a
  //      lone Craftable that IS the base stays a plain leaf (nothing to
  //      expand into beyond its own recipe).
  //
  // Recipes are not part of any key — the panel shows the first
  // member's recipe; SC 4.8 data has every same-model variant (and
  // every duplicate BP) on the same recipe.

  // `Craftable` + the entity collapse live in `$lib/catalog` (shared with the
  // wishlist page). The catalog's second collapse — variant/model bundling
  // into Leaf/Bundle rows — is rendering-shaped and stays here.

  type Leaf = {
    kind: "leaf";
    item: Craftable;
    /** Row title — base item name for gear, else the item's own name. */
    baseName: string;
    sortName: string;
  };
  type Bundle = {
    kind: "bundle";
    /** Base item name — the bundle's row title. */
    baseName: string;
    /** Members sorted by name length (shortest first → base on top). */
    items: Craftable[];
    /** Sort key for the parent list (== baseName). */
    sortName: string;
    /** Stable expand-set key, independent of render order. */
    expandKey: string;
    /** Recipe shown in the expansion (first member's — all members share it). */
    recipe: BpView["recipe"];
  };
  type GroupItem = Leaf | Bundle;

  /** Collapse #2 — group Craftables into per-model bundles / base-titled leaves.
   *  (Collapse #1, the per-entity fold, is `collapseCraftables` in $lib/catalog.) */
  function bundleItems(items: BpView[]): GroupItem[] {
    const craftables = collapseCraftables(items);
    const byModel = new Map<string, Craftable[]>();
    const unkeyed: Craftable[] = []; // no family_id — singletons under own name
    for (const c of craftables) {
      const fid = c.rep.family_id;
      if (!fid) {
        unkeyed.push(c);
        continue;
      }
      const arr = byModel.get(fid) ?? [];
      arr.push(c);
      byModel.set(fid, arr);
    }

    const out: GroupItem[] = [];
    for (const arr of byModel.values()) {
      // Base item name from sc-items (identical across a model's members, so
      // read it before sorting); falls back to the shortest blueprinted name.
      const shortest = arr.reduce((a, b) =>
        nameOf(b.rep).length < nameOf(a.rep).length ? b : a,
      );
      const baseName = arr[0].rep.family_base_name ?? nameOf(shortest.rep);
      // Base member first — identified by name, NOT by length (a colorway can
      // be shorter than the base). Remaining members shortest-first.
      const sorted = [...arr].sort((a, b) => {
        const aBase = nameOf(a.rep) === baseName;
        const bBase = nameOf(b.rep) === baseName;
        if (aBase !== bBase) return aBase ? -1 : 1;
        return (
          nameOf(a.rep).length - nameOf(b.rep).length ||
          nameOf(a.rep).localeCompare(nameOf(b.rep))
        );
      });
      // A lone member that IS the base → plain leaf (no variants to manage,
      // nothing to expand into beyond its own recipe). Everything else —
      // multiple variants, OR a single colorway with no base BP — renders
      // as a bundle so the variant-only case looks like a normal grouping
      // (0/1 count, "Standard" simply absent from the expansion).
      const loneBase =
        sorted.length === 1 &&
        variantSuffix(nameOf(sorted[0].rep), baseName) === "Standard";
      if (loneBase) {
        out.push({ kind: "leaf", item: sorted[0], baseName, sortName: baseName });
      } else {
        out.push({
          kind: "bundle",
          baseName,
          items: sorted,
          sortName: baseName,
          expandKey: "bundle:" + sorted.map((c) => c.entityKey).slice().sort().join(","),
          recipe: sorted[0].rep.recipe,
        });
      }
    }
    for (const c of unkeyed) {
      out.push({
        kind: "leaf",
        item: c,
        baseName: nameOf(c.rep),
        sortName: nameOf(c.rep),
      });
    }
    out.sort((a, b) => a.sortName.localeCompare(b.sortName));
    return out;
  }

  onMount(async () => {
    // Fast, fire-and-forget: learn whether the live-sync button should show.
    // Doesn't block the (slow) catalog load below.
    commands.getSettings().then((r) => {
      if (r.status === "ok") {
        liveSync = { available: r.data.live_sync_available, enabled: r.data.live_sync_enabled };
      }
    });

    const [bpErr] = await Promise.all([ensureBlueprints(), ensureOwnership()]);
    if (bpErr) errorMessage = bpErr;
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
      if (result.data) {
        owned.add(guid);
        // Owning clears want-blueprint server-side (see add_owned); mirror
        // it locally so the count/filter update without a refetch.
        wishRecipe.delete(guid);
      } else owned.delete(guid);
    } else {
      // Revert.
      if (wasOwned) owned.add(guid);
      else owned.delete(guid);
      errorMessage = errText(result.error);
    }
  }

  /** Optimistic flip of one wishlist intent for one BP record. */
  async function toggleWishlist(guid: string, intent: WishIntent) {
    const set = wishSet(intent);
    const wasWanted = set.has(guid);
    if (wasWanted) set.delete(guid);
    else set.add(guid);

    const result = await commands.toggleWishlist(guid, intent);
    if (result.status === "ok") {
      if (result.data) set.add(guid);
      else set.delete(guid);
    } else {
      if (wasWanted) set.add(guid);
      else set.delete(guid);
      errorMessage = errText(result.error);
    }
  }

  const ownedCount = $derived(
    blueprints.filter((b) => owned.has(b.blueprint_record_guid)).length,
  );

  const wantBpCount = $derived(
    blueprints.filter((b) => wishRecipe.has(b.blueprint_record_guid)).length,
  );
  const wantItemCount = $derived(
    blueprints.filter((b) => wishItem.has(b.blueprint_record_guid)).length,
  );

  const filtered = $derived.by(() => {
    const q = query.toLowerCase().trim();
    return blueprints.filter((bp) => {
      const guid = bp.blueprint_record_guid;
      const isOwned = owned.has(guid);
      if (filter === "owned" && !isOwned) return false;
      if (filter === "unowned" && isOwned) return false;
      if (filter === "wantbp" && !wishRecipe.has(guid)) return false;
      if (filter === "wantitem" && !wishItem.has(guid)) return false;
      if (q) {
        const name = bp.display_name?.toLowerCase() ?? "";
        const cat = categoryFor(bp.category_raw, bp.item_type, bp.item_sub_type);
        const catText = `${cat.main} ${cat.sub}`.toLowerCase();
        if (!(name.includes(q) || guid.toLowerCase().includes(q) || catText.includes(q)))
          return false;
      }
      return true;
    });
  });

  // Two-level grouping by sc-crafting Category (primary axis) and the
  // AttachDef item_type / sub_type (secondary axis). Within each
  // subgroup, BPs are collapsed by crafted entity and bundled by model
  // so skin variants and duplicate blueprints fold into one row. See
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

  /** A craftable is owned if ANY of its interchangeable BPs is owned. */
  function craftableOwned(c: Craftable): boolean {
    for (const g of c.bpGuids) if (owned.has(g)) return true;
    return false;
  }

  /** Toggle a craftable's ownership, entity-level (all-or-nothing). When an
   *  item is craftable by several interchangeable BPs, the user can't tell
   *  them apart — in-game they only see the result item — so "owned" means
   *  "I own a BP that crafts this", and we mark/clear every BP together. This
   *  keeps the set internally consistent (no arbitrary "which record", no
   *  partial state) since the read side already treats any-owned as owned. */
  async function toggleCraftable(c: Craftable) {
    const ownedGuids = c.bpGuids.filter((g) => owned.has(g));
    // Owned → clear all; unowned → mark all.
    const targets = ownedGuids.length > 0 ? ownedGuids : c.bpGuids;
    for (const g of targets) await toggleOwned(g);
  }

  /** Owned-variant count for a bundle (craftables, not BPs), reactive on `owned`. */
  function bundleOwnedCount(b: Bundle): number {
    let n = 0;
    for (const c of b.items) if (craftableOwned(c)) n++;
    return n;
  }

  /** A craftable is wishlisted (for an intent) if ANY of its BPs is. */
  function craftableWishes(c: Craftable, intent: WishIntent): boolean {
    const set = wishSet(intent);
    for (const g of c.bpGuids) if (set.has(g)) return true;
    return false;
  }

  /** Toggle a wishlist intent for a craftable, entity-level (all-or-nothing),
   *  mirroring ownership — the interchangeable BPs are indistinguishable to
   *  the user, so wanting "this item" wants every BP that crafts it. */
  async function toggleCraftableWish(c: Craftable, intent: WishIntent) {
    const set = wishSet(intent);
    const wantedGuids = c.bpGuids.filter((g) => set.has(g));
    const targets = wantedGuids.length > 0 ? wantedGuids : c.bpGuids;
    for (const g of targets) await toggleWishlist(g, intent);
  }

  const filters: { id: Filter; label: string; icon?: string }[] = [
    { id: "all", label: "All" },
    { id: "owned", label: "Owned" },
    { id: "unowned", label: "Unowned" },
    { id: "wantbp", label: "Want BP", icon: "⚐" },
    { id: "wantitem", label: "Want item", icon: "♡" },
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
    placeholder="Search name or type…"
    bind:value={query}
    disabled={loading}
  />
  {#if liveSync?.available && liveSync.enabled}
    <button
      class="sync-btn"
      class:syncing
      onclick={syncNow}
      disabled={syncing || loading}
      title="Sync owned blueprints from your account"
      aria-label="Sync owned blueprints"
    >
      <svg
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
      >
        <path d="M23 4v6h-6" />
        <path d="M1 20v-6h6" />
        <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" />
      </svg>
    </button>
  {/if}
</header>

{#if loading}
  <Loading />
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
          {#if f.icon}<span class="chip-icon" aria-hidden="true">{f.icon}</span>{/if}
          {f.label}
          {#if f.id === "owned"}<span class="chip-n">{ownedCount}</span>{/if}
          {#if f.id === "wantbp"}<span class="chip-n">{wantBpCount}</span>{/if}
          {#if f.id === "wantitem"}<span class="chip-n">{wantItemCount}</span>{/if}
        </button>
      {/each}
    </div>
    <div class="legend">
      <span class="legend-item"><span class="legend-icon own">✓</span> own BP</span>
      <span class="legend-item"><span class="legend-icon wish">⚐</span> want BP</span>
      <span class="legend-item"><span class="legend-icon wish">♡</span> want item</span>
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
              {#each subGroup.items as item (item.kind === "leaf" ? item.item.entityKey : item.expandKey)}
                {#if item.kind === "leaf"}
                  {@const c = item.item}
                  {@const bp = c.rep}
                  {@const isOwned = craftableOwned(c)}
                  {@const isExpanded = expanded.has(c.entityKey)}
                  <li class:owned={isOwned} class:expanded={isExpanded}>
                    <div class="bp-row">
                      <!-- Fixed-width status column so leaf checkmarks line up
                           with bundle "x/y" counts (same 2.4rem footprint). -->
                      <span class="own-col">
                        <button
                          class="own-toggle"
                          class:on={isOwned}
                          title={isOwned ? "Blueprint owned — click to unmark" : "Mark blueprint owned"}
                          onclick={() => toggleCraftable(c)}
                        >
                          {isOwned ? "✓" : ""}
                        </button>
                      </span>
                      <button
                        class="bp-expand"
                        title={isExpanded ? "Hide recipe" : "Show recipe"}
                        onclick={() => toggleExpanded(c.entityKey)}
                      >
                        <span class="chevron" class:open={isExpanded} aria-hidden="true">▸</span>
                        <span class="bp-name">{item.baseName}</span>
                        {#if c.bpGuids.length > 1}
                          <span class="dup-tag" title="{c.bpGuids.length} interchangeable blueprints craft this item — marking owned covers all of them">{c.bpGuids.length} BPs</span>
                        {/if}
                        {#if bp.recipe?.craft_time_seconds}
                          <span class="bp-time" title="Craft time">⏱ {formatCraftTime(bp.recipe.craft_time_seconds)}</span>
                        {/if}
                      </button>

                      <!-- Wishlist: ⚐ want the blueprint (only while unowned —
                           owning it means you have the recipe), ♡ want a
                           crafted copy (regardless of ownership). -->
                      <div class="wish-group">
                        {#if !isOwned}
                          {@const wantBp = craftableWishes(c, "recipe")}
                          <button
                            class="wish"
                            class:on={wantBp}
                            title={wantBp ? "Remove blueprint from wishlist" : "Want blueprint (acquire via mission rewards)"}
                            onclick={() => toggleCraftableWish(c, "recipe")}
                          >⚐</button>
                        {:else}
                          <span class="wish placeholder-slot" title="Blueprint owned">·</span>
                        {/if}
                        <button
                          class="wish"
                          class:on={craftableWishes(c, "item")}
                          title={craftableWishes(c, "item") ? "Remove item from wishlist" : "Want a crafted copy of this item"}
                          onclick={() => toggleCraftableWish(c, "item")}
                        >♡</button>
                      </div>
                    </div>

                    {#if isExpanded}
                      <div class="recipe-panel">
                        {#if bp.recipe}
                          {#if bp.recipe.ingredients.length > 0}
                            <ul class="ingredients">
                              <!-- Composite key with index: some recipes legitimately list
                                   the same resource GUID twice (different piles / qualities),
                                   so the bare GUID isn't unique. -->
                              {#each bp.recipe.ingredients as ing, i (`${ing.resource_guid}|${i}`)}
                                <li class="ingredient">
                                  <span class="ing-qty">{formatScu(ing.quantity_scu)} <span class="ing-unit">SCU</span></span>
                                  <span class="ing-name">{ing.resource_name ?? "Unknown resource"}</span>
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
                  {@const totalN = bundle.items.length}
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
                            {#each bundle.recipe.ingredients as ing, i (`${ing.resource_guid}|${i}`)}
                              <li class="ingredient">
                                <span class="ing-qty">{formatScu(ing.quantity_scu)} <span class="ing-unit">SCU</span></span>
                                <span class="ing-name">{ing.resource_name ?? "Unknown resource"}</span>
                                {#if ing.min_quality > 0}
                                  <span class="ing-quality" title="Minimum required quality">≥ Q{ing.min_quality}</span>
                                {/if}
                              </li>
                            {/each}
                          </ul>
                        {/if}
                        <div class="variant-list">
                          {#each bundle.items as vc (vc.entityKey)}
                            {@const vOwned = craftableOwned(vc)}
                            {@const suffix = variantSuffix(nameOf(vc.rep), bundle.baseName)}
                            {@const isBase = suffix === "Standard"}
                            <div class="variant" class:owned={vOwned}>
                              <button
                                class="own-toggle"
                                class:on={vOwned}
                                title={vOwned ? "Blueprint owned — click to unmark" : "Mark blueprint owned"}
                                onclick={() => toggleCraftable(vc)}
                              >
                                {vOwned ? "✓" : ""}
                              </button>
                              <span class="variant-name" class:base={isBase}>{suffix}</span>
                              {#if vc.bpGuids.length > 1}
                                <span class="dup-tag" title="{vc.bpGuids.length} interchangeable blueprints craft this item — marking owned covers all of them">{vc.bpGuids.length} BPs</span>
                              {/if}
                              <div class="wish-group">
                                {#if !vOwned}
                                  {@const vWantBp = craftableWishes(vc, "recipe")}
                                  <button
                                    class="wish"
                                    class:on={vWantBp}
                                    title={vWantBp ? "Remove blueprint from wishlist" : "Want blueprint (acquire via mission rewards)"}
                                    onclick={() => toggleCraftableWish(vc, "recipe")}
                                  >⚐</button>
                                {:else}
                                  <span class="wish placeholder-slot" title="Blueprint owned">·</span>
                                {/if}
                                <button
                                  class="wish"
                                  class:on={craftableWishes(vc, "item")}
                                  title={craftableWishes(vc, "item") ? "Remove item from wishlist" : "Want a crafted copy of this item"}
                                  onclick={() => toggleCraftableWish(vc, "item")}
                                >♡</button>
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

  .error {
    padding: 1rem 1.6rem;
    color: var(--muted);
  }
  .error strong {
    color: var(--bad);
  }

  /* Spin animation — used by the live-sync button below while syncing. */
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  /* Catalog live-sync button — only shown when live sync is enabled. */
  .sync-btn {
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    width: 2rem;
    height: 2rem;
    border-radius: 7px;
    background: transparent;
    border: 1px solid var(--line);
    color: var(--muted);
    cursor: pointer;
    transition: color 90ms, border-color 90ms;
  }
  .sync-btn:hover:not(:disabled) {
    color: var(--ember);
    border-color: var(--ember-dim);
  }
  .sync-btn:disabled {
    opacity: 0.5;
    cursor: progress;
  }
  .sync-btn svg {
    width: 1rem;
    height: 1rem;
    display: block;
  }
  .sync-btn.syncing svg {
    animation: spin 0.8s linear infinite;
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
  .chip-icon {
    font-size: 0.9rem;
    line-height: 1;
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
  .legend-icon.wish {
    color: var(--muted);
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
  /* Status column — same 2.4rem footprint as a bundle's "x/y" count, so
     leaf checkmarks and bundle counts share one left edge and the names
     below them line up. The checkmark stays its 1.4rem square, centred. */
  .own-col {
    flex: 0 0 2.4rem;
    display: grid;
    place-items: center;
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
  .bp-time {
    flex: 0 0 auto;
    font-size: 0.72rem;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    padding: 0.1rem 0.45rem;
    border-radius: 999px;
    background: var(--panel-2);
  }
  /* Marker for an item craftable by several interchangeable blueprints. */
  .dup-tag {
    flex: 0 0 auto;
    font-size: 0.62rem;
    color: var(--muted);
    background: var(--panel-2);
    border: 1px solid var(--line);
    padding: 0.05rem 0.4rem;
    border-radius: 4px;
    font-variant-numeric: tabular-nums;
  }
  .wish-group {
    display: flex;
    align-items: center;
    gap: 0.1rem;
  }
  /* Wishlist toggles — ⚐ want-blueprint, ♡ want-item. Button reset; faint
     until active, ember when on. */
  button.wish {
    background: transparent;
    border: none;
    cursor: pointer;
  }
  .wish {
    font-size: 1.05rem;
    line-height: 1;
    padding: 0.25rem 0.3rem;
    color: var(--faint);
    transition: color 90ms, transform 90ms;
  }
  button.wish:hover {
    color: var(--muted);
    transform: scale(1.12);
  }
  button.wish.on {
    color: var(--ember);
  }
  /* Keeps the ♡ aligned when the ⚐ slot is suppressed (BP owned). */
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
    background: linear-gradient(90deg, var(--ember-glow), transparent 60%);
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
  .variant .wish {
    font-size: 0.95rem;
    padding: 0.15rem 0.25rem;
  }
</style>
