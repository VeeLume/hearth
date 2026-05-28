<script lang="ts">
  import { onMount } from "svelte";
  import { commands, type BpView } from "$lib/bindings";
  import { categoryFor } from "$lib/itemTypes";

  let blueprints = $state<BpView[]>([]);
  let owned = $state<Set<string>>(new Set());
  let loading = $state(true);
  let errorMessage = $state<string | null>(null);
  let query = $state("");

  type Filter = "all" | "owned" | "unowned";
  let filter = $state<Filter>("all");

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
      owned = new Set(ownedResult.data.map((o) => o.blueprint_guid));
    }
    loading = false;
  });

  async function toggleOwned(guid: string) {
    // Optimistic flip, reconcile from the command's returned truth.
    const next = new Set(owned);
    next.has(guid) ? next.delete(guid) : next.add(guid);
    owned = next;
    const result = await commands.toggleOwned(guid);
    if (result.status === "ok") {
      const reconciled = new Set(owned);
      result.data ? reconciled.add(guid) : reconciled.delete(guid);
      owned = reconciled;
    } else {
      // Revert on failure.
      const reverted = new Set(owned);
      reverted.has(guid) ? reverted.delete(guid) : reverted.add(guid);
      owned = reverted;
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
        const cat = categoryFor(bp.item_type, bp.item_sub_type);
        const catText = `${cat.main} ${cat.sub}`.toLowerCase();
        if (!(name.includes(q) || guid.includes(q) || catText.includes(q)))
          return false;
      }
      return true;
    });
  });

  // Two-level grouping by friendly item-type taxonomy (not blueprint
  // reward pool — pools are a mission-reward mechanic). categoryFor maps
  // the raw BpView.item_type into a main + sub category.
  type SubGroup = { sub: string; subOrder: number; items: BpView[] };
  type MainGroup = {
    main: string;
    mainOrder: number;
    total: number;
    subs: SubGroup[];
  };

  const grouped = $derived.by((): MainGroup[] => {
    const mains = new Map<string, MainGroup & { subMap: Map<string, SubGroup> }>();
    for (const bp of filtered) {
      const cat = categoryFor(bp.item_type, bp.item_sub_type);
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
        sub = { sub: cat.sub, subOrder: cat.subOrder, items: [] };
        main.subMap.set(cat.sub, sub);
        main.subs.push(sub);
      }
      sub.items.push(bp);
      main.total += 1;
    }
    const byName = (a: BpView, b: BpView) =>
      (a.display_name ?? a.blueprint_record_guid).localeCompare(
        b.display_name ?? b.blueprint_record_guid,
      );
    const out = [...mains.values()];
    for (const m of out) {
      for (const s of m.subs) s.items.sort(byName);
      m.subs.sort((a, b) => a.subOrder - b.subOrder || a.sub.localeCompare(b.sub));
    }
    out.sort((a, b) => a.mainOrder - b.mainOrder || a.main.localeCompare(b.main));
    return out;
  });

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
    Loading SC reference data… (first run takes ~10 s while the Datacore is
    parsed; subsequent loads are instant)
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
              {#each subGroup.items as bp, i (`${bp.blueprint_record_guid}|${bp.crafted_entity_guid ?? ""}|${i}`)}
                {@const isOwned = owned.has(bp.blueprint_record_guid)}
                <li class:owned={isOwned}>
                  <button
                    class="own-toggle"
                    class:on={isOwned}
                    title={isOwned ? "Blueprint owned — click to unmark" : "Mark blueprint owned"}
                    onclick={() => toggleOwned(bp.blueprint_record_guid)}
                  >
                    {isOwned ? "✓" : ""}
                  </button>
                  <span class="bp-name">{bp.display_name ?? bp.blueprint_record_guid}</span>
                  {#if bp.display_name}
                    <span class="bp-guid">{bp.blueprint_record_guid}</span>
                  {/if}

                  <!-- Wishlist intents — present but disabled until Stage 7. -->
                  <div class="wish-group">
                    {#if !isOwned}
                      <span class="wish soon" title="Want blueprint — coming in a later version">⚐</span>
                    {:else}
                      <span class="wish placeholder-slot">·</span>
                    {/if}
                    <span class="wish soon" title="Want crafted item — coming in a later version">♡</span>
                  </div>
                </li>
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
    display: flex;
    align-items: center;
    gap: 0.8rem;
    padding: 0.5rem 0.6rem;
    border-radius: 8px;
    border: 1px solid transparent;
  }
  li:hover {
    background: var(--panel);
  }
  li.owned {
    background: linear-gradient(90deg, var(--ember-glow), transparent 60%);
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
  .bp-name {
    flex: 1;
    font-size: 0.9rem;
  }
  .bp-guid {
    font-family: ui-monospace, "SF Mono", Consolas, monospace;
    font-size: 0.72rem;
    color: var(--faint);
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
</style>
