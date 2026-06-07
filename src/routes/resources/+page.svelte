<script lang="ts">
  import { onMount } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { commands, errText, type AppSettings, type InventoryStack } from "$lib/ipc";
  import { data, ensureInventory, refreshInventory } from "$lib/state/data.svelte";
  import { stackLocationLabel } from "$lib/domain/inventory";
  import { formatScu } from "$lib/domain/catalog";
  import Loading from "$lib/components/Loading.svelte";
  import PageHeader from "$lib/components/PageHeader.svelte";
  import SyncButton from "$lib/components/SyncButton.svelte";

  // Your live resource inventory, read from CIG's backend via the optional
  // resource sync (Settings → Blueprint import → Live resource sync). The page
  // reads the persisted snapshot from the shared store, so it works offline and
  // updates in place when a sync reconciles it.

  let loading = $state(!data.inventoryReady);
  let errorMessage = $state<string | null>(null);
  let settings = $state<AppSettings | null>(null);
  let syncing = $state(false);

  onMount(async () => {
    const [err, s] = await Promise.all([ensureInventory(), commands.getSettings()]);
    if (err) errorMessage = err;
    if (s.status === "ok") settings = s.data;
    loading = false;
  });

  const inventory = $derived(data.inventory);
  const syncedAt = $derived(inventory.length ? inventory[0].synced_at : null);

  type Group = {
    crc: number;
    kind: InventoryStack["kind"];
    name: string;
    /** Total SCU (resources) or total unit count (items). */
    total: number;
    bestQuality: number | null;
    stacks: InventoryStack[];
  };

  // Group every stack by material/item, aggregating the amount and surfacing the
  // best quality + the per-quality / per-location breakdown. Resources and items
  // are grouped identically — only the amount unit (SCU vs ×count) differs.
  function groupBy(stacks: InventoryStack[]): Group[] {
    const m = new Map<number, Group>();
    for (const s of stacks) {
      let g = m.get(s.crc);
      if (!g) {
        g = {
          crc: s.crc,
          kind: s.kind,
          name: s.name ?? `Unknown (${s.crc})`,
          total: 0,
          bestQuality: null,
          stacks: [],
        };
        m.set(s.crc, g);
      }
      g.total += s.kind === "item" ? (s.count ?? 0) : (s.scu ?? 0);
      if (s.quality != null && (g.bestQuality == null || s.quality > g.bestQuality)) {
        g.bestQuality = s.quality;
      }
      g.stacks.push(s);
    }
    for (const g of m.values()) {
      // Finest grain first: highest quality, then by location.
      g.stacks.sort(
        (a, b) =>
          (b.quality ?? 0) - (a.quality ?? 0) ||
          stackLocationLabel(a).localeCompare(stackLocationLabel(b)),
      );
    }
    return [...m.values()].sort((a, b) => a.name.localeCompare(b.name));
  }

  // One list: resources and crafting items grouped together and sorted by name,
  // interleaved in normal order (no separate section).
  const groups = $derived(groupBy(inventory));
  const materialCount = $derived(groups.filter((g) => g.kind === "resource").length);
  const itemCount = $derived(groups.filter((g) => g.kind === "item").length);

  // Which materials are expanded to their per-quality / per-location breakdown.
  const expanded = new SvelteSet<number>();
  function toggle(crc: number) {
    if (expanded.has(crc)) expanded.delete(crc);
    else expanded.add(crc);
  }

  // Quality is reported on a 0..1000 scale; show it as a compact "Q###".
  const qualityLabel = (q: number | null) => (q == null ? "—" : `Q${q}`);

  const canSync = $derived(!!settings?.live_sync_available);
  const inventoryOn = $derived(!!settings?.live_inventory_enabled);
  const online = $derived(!!settings?.online_enabled);

  async function syncNow() {
    if (syncing) return;
    syncing = true;
    errorMessage = null;
    const r = await commands.inventorySyncNow();
    if (r.status === "error") errorMessage = errText(r.error);
    await refreshInventory();
    syncing = false;
  }

  const fmtSynced = (ts: string) => new Date(ts).toLocaleString();
</script>

<PageHeader title="Resources">
  {#snippet subtitle()}
    {#if loading}Loading…{:else}{materialCount} material{materialCount === 1 ? "" : "s"}{#if itemCount} · {itemCount} crafting item{itemCount === 1 ? "" : "s"}{/if}{/if}
  {/snippet}
  <a class="craft-link" href="/crafting">What can I craft →</a>
</PageHeader>

{#if loading}
  <Loading />
{:else}
  <section class="res">
    <div class="bar">
      <div class="bar-left">
        {#if syncedAt}
          <span class="synced">Last synced {fmtSynced(syncedAt)}</span>
        {:else}
          <span class="synced muted">Never synced</span>
        {/if}
      </div>
      <div class="bar-right">
        {#if canSync}
          <SyncButton
            onclick={syncNow}
            syncing={syncing}
            disabled={!inventoryOn || !online}
            title={!inventoryOn
              ? "Enable resource sync in Settings to use this"
              : !online
                ? "Offline mode is on — turn it off in Settings"
                : "Sync your resource inventory from your account"}
            label="Sync resources"
          />
        {/if}
      </div>
    </div>

    {#if errorMessage}
      <div class="error"><strong>Sync failed.</strong> {errorMessage}</div>
    {/if}

    {#if !canSync}
      <div class="hint">Live resource sync isn't available in this build.</div>
    {:else if inventory.length === 0}
      <div class="hint">
        {#if !inventoryOn}
          No resources yet. Enable <a href="/settings">Live resource sync</a> to read
          your in-game inventory.
        {:else if !online}
          Offline mode is on — turn it off in <a href="/settings">Settings</a> to sync.
        {:else}
          No resources synced yet. Press <strong>Sync now</strong> to read your
          in-game inventory.
        {/if}
      </div>
    {:else}
      <ul class="groups">
        {#each groups as g (g.crc)}{@render groupRow(g)}{/each}
      </ul>
    {/if}
  </section>
{/if}

<!-- One expandable row, shared by the materials and crafting-items lists. The
     amount renders as SCU for resources and ×count for items. -->
{#snippet groupRow(g: Group)}
  {@const isOpen = expanded.has(g.crc)}
  <li class="group" class:open={isOpen}>
    <button class="g-head" onclick={() => toggle(g.crc)} aria-expanded={isOpen}>
      <span class="chev" class:open={isOpen} aria-hidden="true">▸</span>
      <span class="g-name">{g.name}</span>
      <span class="g-scu">
        {#if g.kind === "item"}×{g.total}{:else}{formatScu(g.total)} <span class="unit">SCU</span>{/if}
      </span>
      <span class="g-q" title="Best available quality (0–1000)">{qualityLabel(g.bestQuality)}</span>
      <span class="g-count">{g.stacks.length} stack{g.stacks.length === 1 ? "" : "s"}</span>
    </button>
    {#if isOpen}
      <ul class="breakdown">
        {#each g.stacks as st (st.id)}
          <li>
            <span class="b-q" title="Quality (0–1000)">{qualityLabel(st.quality)}</span>
            <span class="b-scu">
              {#if g.kind === "item"}×{st.count ?? 0}{:else}{formatScu(st.scu ?? 0)} <span class="unit">SCU</span>{/if}
            </span>
            <span class="b-loc">{stackLocationLabel(st)}</span>
          </li>
        {/each}
      </ul>
    {/if}
  </li>
{/snippet}

<style>
  /* Header cross-link to the crafting planner (right-aligned in PageHeader). */
  .craft-link {
    margin-left: auto;
    font-size: 0.8rem;
    color: var(--ember);
    text-decoration: none;
    white-space: nowrap;
  }
  .craft-link:hover {
    text-decoration: underline;
  }
  .res {
    flex: 1;
    overflow-y: auto;
    padding: 0.5rem 1.6rem 2rem;
  }
  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.4rem 0.2rem 0.8rem;
    gap: 1rem;
  }
  .synced {
    font-size: 0.78rem;
    color: var(--muted);
  }
  .synced.muted {
    color: var(--faint);
    font-style: italic;
  }
  .error {
    margin: 0 0.2rem 0.8rem;
    padding: 0.5rem 0.7rem;
    border-radius: 8px;
    font-size: 0.82rem;
    color: var(--bad);
    background: var(--panel);
    border: 1px solid var(--line);
  }
  .error strong {
    color: var(--bad);
  }
  .hint {
    margin: 1rem 0.2rem;
    font-size: 0.85rem;
    color: var(--faint);
    font-style: italic;
  }
  .hint a {
    color: var(--ember);
  }
  ul.groups {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .group {
    border-radius: 8px;
    border: 1px solid transparent;
  }
  .group:hover {
    background: var(--panel);
  }
  .group.open {
    background: var(--panel);
    border-color: var(--line);
  }
  .g-head {
    width: 100%;
    display: grid;
    grid-template-columns: 1rem minmax(8rem, 1.6fr) 7rem 3.2rem 4.5rem;
    align-items: center;
    gap: 0.8rem;
    padding: 0.45rem 0.6rem;
    background: transparent;
    border: none;
    color: var(--text);
    font-size: 0.88rem;
    text-align: left;
    cursor: pointer;
  }
  .chev {
    display: inline-block;
    font-size: 0.7em;
    color: var(--faint);
    transition: transform 120ms;
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .g-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .g-scu {
    color: var(--ember);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .g-scu .unit {
    color: var(--faint);
    font-size: 0.8em;
  }
  .g-q {
    font-size: 0.78rem;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .g-count {
    font-size: 0.72rem;
    color: var(--faint);
    text-align: right;
  }

  /* Per-quality / per-location breakdown under an expanded material. */
  .breakdown {
    list-style: none;
    margin: 0;
    padding: 0 0.6rem 0.5rem 1.6rem;
  }
  .breakdown li {
    display: grid;
    grid-template-columns: 3.5rem 7rem 1fr;
    align-items: baseline;
    gap: 0.8rem;
    padding: 0.2rem 0;
    font-size: 0.8rem;
    border-top: 1px dashed var(--line);
  }
  .b-q {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .b-scu {
    color: var(--ember);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .b-scu .unit {
    color: var(--faint);
    font-size: 0.8em;
  }
  .b-loc {
    color: var(--muted);
  }
</style>
