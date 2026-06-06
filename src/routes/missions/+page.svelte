<script lang="ts">
  import { onMount, tick } from "svelte";
  import { page } from "$app/state";
  import { SvelteSet, SvelteMap } from "svelte/reactivity";
  import { type MissionView } from "$lib/ipc";
  import Loading from "$lib/components/Loading.svelte";
  import PageHeader from "$lib/components/PageHeader.svelte";
  import {
    data,
    owned,
    ensureMissions,
    ensureOwnership,
    toggleOwned as setOwned,
  } from "$lib/state/data.svelte";

  // Missions through the blueprint lens. Hearth is BP-focused, so a mission's
  // tracked value is "can it still give me a blueprint I don't own?". There's
  // no manual "done" toggle — a mission is *exhausted* when you own every BP
  // across its reward pool(s), derived from the catalog's owned set. A reward's
  // checkbox IS the catalog ownership toggle (collecting it = owning the BP).

  // Missions + ownership come from the shared store ($lib/data.svelte) so
  // navigation doesn't re-fetch. `owned` is the shared set (imported).
  const missions = $derived(data.missions);
  let loading = $state(!(data.missionsReady && data.ownershipReady));
  let errorMessage = $state<string | null>(null);
  let query = $state("");
  let expanded = new SvelteSet<string>();
  // Per-mission selected "available in" locality card (index into m.locations).
  let selectedLoc = new SvelteMap<string, number>();

  // Group a locality's places into ordered, labelled buckets for the
  // expanded card. Lagrange points are detected by their `L<n>` name since
  // they don't carry a distinct kind.
  type Place = MissionView["locations"][number]["places"][number];
  const KIND_BUCKETS: { label: string; kinds: string[] }[] = [
    { label: "Planets", kinds: ["Planet", "S42_Planet"] },
    { label: "Moons", kinds: ["Moon", "S42_Moon"] },
    { label: "Stations", kinds: ["Manmade", "Manmade_VisibleOnInteraction", "ManmadeJumpPoint"] },
    { label: "Outposts", kinds: ["Outpost", "Outpost_InvalidQT"] },
    { label: "Asteroid belts / clusters", kinds: ["Asteroid", "Asteroid_ValidQT"] },
  ];
  function placeBucket(p: Place): string {
    const rec = p.record_name ?? "";
    if (/(^|[ _])L[1-5]\b/i.test(rec) || /(^|[ _])L[1-5]\b/i.test(p.name ?? "")) return "Lagrange";
    const k = p.kind ?? "";
    for (const b of KIND_BUCKETS) if (b.kinds.includes(k)) return b.label;
    return "Other";
  }
  function kindBuckets(places: Place[]): { label: string; places: Place[] }[] {
    const order = [...KIND_BUCKETS.map((b) => b.label), "Lagrange", "Other"];
    const by = new Map<string, Place[]>();
    for (const p of places) {
      const b = placeBucket(p);
      (by.get(b) ?? by.set(b, []).get(b)!).push(p);
    }
    return order.filter((l) => by.has(l)).map((label) => ({ label, places: by.get(label)! }));
  }
  // No card open by default — the breakdown can get long (Pyro), so it's
  // collapsed until a chip is clicked. Clicking the open chip collapses it.
  const selectedRegion = (m: MissionView) => {
    const idx = selectedLoc.get(m.mission_id);
    return idx == null ? undefined : m.locations[idx];
  };
  function toggleLoc(missionId: string, ri: number) {
    if (selectedLoc.get(missionId) === ri) selectedLoc.delete(missionId);
    else selectedLoc.set(missionId, ri);
  }

  // Encounter ship-candidate lists are long (10-30 models); collapsed by
  // default, expanded per slot on demand.
  let expandedShips = new SvelteSet<string>();
  function toggleSet(set: SvelteSet<string>, key: string) {
    if (set.has(key)) set.delete(key);
    else set.add(key);
  }
  const shipRange = (slot: { count_min: number; count_max: number }): string =>
    slot.count_min === slot.count_max ? `${slot.count_min}` : `${slot.count_min}-${slot.count_max}`;

  type Filter = "grantsbp" | "all" | "outstanding" | "exhausted";
  let filter = $state<Filter>("grantsbp");
  let factionFilter = $state<string | "">("");
  let categoryFilter = $state<string | "">("");
  let systemFilter = $state<string | "">("");
  // Required-reputation tier window (indices on the 0–6 FactionRep scale);
  // "" = unbounded. Groundwork for the sc-dossier "what can I accept" filter.
  let repMin = $state<number | "">("");
  let repMax = $state<number | "">("");

  /** List sort: title (A–Z) or by the visible aUEC payout (either direction). */
  let sortMode = $state<"title" | "payout-desc" | "payout-asc">("title");

  /** Collapse same-title variants into one "mission family" row (toggle, on by default). */
  let grouped = $state(true);
  let expandedFamilies = new SvelteSet<string>();

  // Distinct facets for the dropdown filters (built once per mission load).
  const factionList = $derived.by(() => {
    const s = new Set<string>();
    for (const m of missions) if (m.faction?.name) s.add(m.faction.name);
    return [...s].sort();
  });
  const categoryList = $derived.by(() => {
    const s = new Set<string>();
    for (const m of missions) if (m.category?.name) s.add(m.category.name);
    return [...s].sort();
  });
  const systemList = $derived.by(() => {
    const s = new Set<string>();
    for (const m of missions) for (const r of m.locations) s.add(r.system);
    return [...s].sort();
  });
  // Reputation tiers present in the data: index → display name, ordered.
  const repRanks = $derived.by(() => {
    const map = new Map<number, string>();
    for (const m of missions)
      for (const r of m.rep_required) {
        if (r.min_rank_index != null && r.min_rank) map.set(r.min_rank_index, r.min_rank);
        if (r.max_rank_index != null && r.max_rank) map.set(r.max_rank_index, r.max_rank);
      }
    return [...map.entries()].sort((a, b) => a[0] - b[0]).map(([i, name]) => ({ i, name }));
  });

  let filtersOpen = $state(false);
  // Count of active facet filters (drives the "Filters ▾" badge). Search is
  // separate (lives in the header).
  const activeFilterCount = $derived(
    (categoryFilter ? 1 : 0) +
      (factionFilter ? 1 : 0) +
      (systemFilter ? 1 : 0) +
      (repMin !== "" || repMax !== "" ? 1 : 0),
  );
  function clearFilters() {
    factionFilter = "";
    categoryFilter = "";
    systemFilter = "";
    repMin = "";
    repMax = "";
    query = "";
  }

  /** True if any (non-exclusion) rep requirement's tier window overlaps
   *  the selected [lo, hi] rank range. */
  function repOverlap(m: MissionView, lo: number, hi: number): boolean {
    return m.rep_required.some((r) => {
      if (r.exclude) return false;
      const rmin = r.min_rank_index ?? 0;
      const rmax = r.max_rank_index ?? 6;
      return rmax >= lo && rmin <= hi;
    });
  }

  /** Jump to a prerequisite mission: expand it + scroll into view, relaxing
   *  filters only if it isn't currently visible. */
  async function jumpToMission(ref: { mission_id: string; title: string | null }) {
    const target =
      missions.find((x) => x.mission_id === ref.mission_id) ??
      (ref.title ? missions.find((x) => (x.title ?? x.debug_name) === ref.title) : undefined);
    if (!target) return;
    const visible = grouped
      ? families.slice(0, visibleCount).some((f) => f.members.includes(target))
      : sorted.slice(0, visibleCount).includes(target);
    if (!visible) {
      filter = "all";
      factionFilter = "";
      categoryFilter = "";
      systemFilter = "";
      repMin = "";
      repMax = "";
      query = target.title ?? target.debug_name;
    }
    expandedFamilies.add(familyKey(target));
    expanded.add(target.mission_id);
    await tick();
    document
      .getElementById(`m-${target.mission_id}`)
      ?.scrollIntoView({ behavior: "smooth", block: "center" });
  }

  /** Incremental render page size — the list reveals this many rows at a time
   *  ("Show more"), so large result sets stay snappy without a hard cap. */
  const PAGE = 300;
  let visibleCount = $state(PAGE);

  onMount(async () => {
    const [mErr] = await Promise.all([ensureMissions(), ensureOwnership()]);
    if (mErr) errorMessage = mErr;
    loading = false;
  });

  function toggleExpanded(id: string) {
    if (expanded.has(id)) expanded.delete(id);
    else expanded.add(id);
  }

  const missionTitle = (m: MissionView) => m.title ?? m.debug_name;
  const grantsBp = (m: MissionView) => m.blueprint_rewards.length > 0;

  /** Distinct star systems where the mission is offered — a compact row hint;
   *  the full per-locality place list shows on expand. */
  const systemsOf = (m: MissionView): string[] => [
    ...new Set(m.locations.map((r) => r.system)),
  ];

  /** Visible aUEC payout: fixed amount, evergr3n estimate, or "calculated". */
  function payoutText(m: MissionView): string {
    const p = m.payout;
    if (p.fixed != null) return `${p.fixed.toLocaleString()} aUEC`;
    if (p.estimate != null) return `~${p.estimate.toLocaleString()} aUEC`;
    if (p.calculated) return "calculated";
    return "—";
  }

  /** Numeric payout for sorting: fixed amount, else estimate, else null
   *  (missions with no known payout sort last regardless of direction). */
  function payoutValue(m: MissionView): number | null {
    return m.payout.fixed ?? m.payout.estimate ?? null;
  }

  /** Rough difficulty tier (avg of the four hidden axes), 0 when unknown. */
  function difficultyTier(m: MissionView): number {
    const d = m.difficulty;
    if (!d) return 0;
    return Math.round(
      (d.mechanical_skill + d.mental_load + d.risk_of_loss + d.game_knowledge) / 4,
    );
  }

  // Distinct reward blueprint guids per mission (built once per mission load).
  const bpGuidsByMission = $derived.by(() => {
    const map = new Map<string, string[]>();
    for (const m of missions) {
      const set = new Set<string>();
      for (const p of m.blueprint_rewards)
        for (const b of p.blueprints) set.add(b.blueprint_record_guid);
      map.set(m.mission_id, [...set]);
    }
    return map;
  });

  const rewardGuids = (m: MissionView) => bpGuidsByMission.get(m.mission_id) ?? [];
  const ownedCountOf = (m: MissionView) => rewardGuids(m).filter((g) => owned.has(g)).length;
  const isExhausted = (m: MissionView) => {
    const g = rewardGuids(m);
    return g.length > 0 && g.every((x) => owned.has(x));
  };
  const hasOutstanding = (m: MissionView) => {
    const g = rewardGuids(m);
    return g.length > 0 && !g.every((x) => owned.has(x));
  };

  /** Toggle catalog ownership of a reward blueprint (optimistic, shared store
   *  action); surface any error inline. */
  async function toggleOwned(guid: string) {
    const err = await setOwned(guid);
    if (err) errorMessage = err;
  }

  const outstandingCount = $derived(missions.filter(hasOutstanding).length);

  // Cross-link from the wishlist: `?bp=<guid,guid,…>` narrows to the missions
  // that grant any of those blueprint records; `?name=` labels the banner. The
  // wishlist passes every interchangeable BP guid of a craftable so this matches
  // whichever record the mission pool happens to reference.
  const bpFilter = $derived.by(() => {
    const raw = page.url.searchParams.get("bp");
    if (!raw) return null;
    const set = new Set(raw.split(",").map((s) => s.trim()).filter(Boolean));
    return set.size > 0 ? set : null;
  });
  const bpFilterName = $derived(page.url.searchParams.get("name"));

  const filtered = $derived.by(() => {
    const q = query.toLowerCase().trim();
    const bp = bpFilter;
    return missions.filter((m) => {
      if (bp && !rewardGuids(m).some((g) => bp.has(g))) return false;
      if (filter === "grantsbp" && !grantsBp(m)) return false;
      if (filter === "outstanding" && !hasOutstanding(m)) return false;
      if (filter === "exhausted" && !isExhausted(m)) return false;
      if (factionFilter && m.faction?.name !== factionFilter) return false;
      if (categoryFilter && m.category?.name !== categoryFilter) return false;
      if (systemFilter && !systemsOf(m).includes(systemFilter)) return false;
      if (repMin !== "" || repMax !== "") {
        const lo = repMin === "" ? 0 : repMin;
        const hi = repMax === "" ? 6 : repMax;
        if (!repOverlap(m, lo, hi)) return false;
      }
      if (q) {
        const t = missionTitle(m).toLowerCase();
        if (!(t.includes(q) || m.mission_id.toLowerCase().includes(q))) return false;
      }
      return true;
    });
  });

  /** `filtered` in the chosen sort order. Title uses locale compare; payout
   *  uses the visible amount with unknown-payout missions sorted last. */
  const sorted = $derived.by(() => {
    const arr = [...filtered];
    // Stable, meaningful tiebreak so equal-key rows never fall back to GUID
    // order (many missions share a title / payout — e.g. the hauling variants).
    const byTitleId = (a: MissionView, b: MissionView) => {
      const t = missionTitle(a).localeCompare(missionTitle(b));
      return t !== 0 ? t : a.mission_id.localeCompare(b.mission_id);
    };
    if (sortMode === "title") {
      arr.sort((a, b) => {
        const t = missionTitle(a).localeCompare(missionTitle(b));
        if (t !== 0) return t;
        // Same title → highest payout first, then a stable id tiebreak.
        const pa = payoutValue(a) ?? -Infinity;
        const pb = payoutValue(b) ?? -Infinity;
        if (pa !== pb) return pb - pa;
        return a.mission_id.localeCompare(b.mission_id);
      });
    } else {
      const dir = sortMode === "payout-asc" ? 1 : -1;
      arr.sort((a, b) => {
        const pa = payoutValue(a);
        const pb = payoutValue(b);
        if (pa == null && pb == null) return byTitleId(a, b);
        if (pa == null) return 1;
        if (pb == null) return -1;
        if (pa !== pb) return dir * (pa - pb);
        return byTitleId(a, b);
      });
    }
    return arr;
  });

  // Reset incremental paging whenever the result set or its order/grouping changes.
  $effect(() => {
    void [query, filter, factionFilter, categoryFilter, systemFilter, repMin, repMax, bpFilter, sortMode, grouped];
    visibleCount = PAGE;
  });

  /** A "mission family" — same-title variants collapsed into one row. Keyed by
   *  title + faction + category (the visible identity); the roll-up is
   *  collection-first (union of distinct reward blueprints, owned/total). */
  type Family = {
    id: string;
    title: string;
    faction: string | null;
    category: string | null;
    members: MissionView[];
    bpGuids: string[];
    ownedCount: number;
    collectable: number;
    payoutLo: number | null;
    payoutHi: number | null;
    systems: string[];
    rep: boolean;
    exhausted: boolean;
  };

  const familyKey = (m: MissionView) =>
    `${missionTitle(m)} ${m.faction?.name ?? ""} ${m.category?.name ?? ""}`;

  /** `filtered` grouped into families, ordered by the active sort. Member rows
   *  within a family lead with the ones that still grant unowned blueprints. */
  const families = $derived.by((): Family[] => {
    const map = new SvelteMap<string, MissionView[]>();
    for (const m of filtered) {
      const k = familyKey(m);
      const arr = map.get(k);
      if (arr) arr.push(m);
      else map.set(k, [m]);
    }
    const fams: Family[] = [...map.values()].map((members) => {
      const bpSet = new Set<string>();
      for (const m of members) for (const g of rewardGuids(m)) bpSet.add(g);
      const bpGuids = [...bpSet];
      const ownedCount = bpGuids.filter((g) => owned.has(g)).length;
      const sys = new Set<string>();
      for (const m of members) for (const s of systemsOf(m)) sys.add(s);
      const vals = members
        .map(payoutValue)
        .filter((v): v is number => v != null);
      // Collection-smart member order: still-collectable first, then payout desc.
      const ordered = [...members].sort((a, b) => {
        const ua = ownedCountOf(a) < rewardGuids(a).length ? 0 : 1;
        const ub = ownedCountOf(b) < rewardGuids(b).length ? 0 : 1;
        if (ua !== ub) return ua - ub;
        const pa = payoutValue(a) ?? -Infinity;
        const pb = payoutValue(b) ?? -Infinity;
        if (pa !== pb) return pb - pa;
        return a.mission_id.localeCompare(b.mission_id);
      });
      return {
        id: familyKey(members[0]),
        title: missionTitle(members[0]),
        faction: members[0].faction?.name ?? null,
        category: members[0].category?.name ?? null,
        members: ordered,
        bpGuids,
        ownedCount,
        collectable: members.filter((m) => ownedCountOf(m) < rewardGuids(m).length).length,
        payoutLo: vals.length ? Math.min(...vals) : null,
        payoutHi: vals.length ? Math.max(...vals) : null,
        systems: [...sys],
        rep: members.some((m) => m.rep_required.length > 0),
        exhausted: bpGuids.length > 0 && ownedCount === bpGuids.length,
      };
    });
    if (sortMode === "title") {
      fams.sort((a, b) => a.title.localeCompare(b.title) || a.id.localeCompare(b.id));
    } else {
      const dir = sortMode === "payout-asc" ? 1 : -1;
      fams.sort((a, b) => {
        const pa = sortMode === "payout-asc" ? (a.payoutLo ?? Infinity) : (a.payoutHi ?? -Infinity);
        const pb = sortMode === "payout-asc" ? (b.payoutLo ?? Infinity) : (b.payoutHi ?? -Infinity);
        if (pa !== pb) return dir * (pa - pb);
        return a.title.localeCompare(b.title);
      });
    }
    return fams;
  });

  /** The count of units currently rendered (families when grouped, else rows). */
  const renderCount = $derived(grouped ? families.length : sorted.length);

  function payoutRange(f: Family): string {
    if (f.payoutLo == null) return "—";
    if (f.payoutLo === f.payoutHi) return `~${f.payoutLo.toLocaleString()} aUEC`;
    return `~${f.payoutLo.toLocaleString()}–${(f.payoutHi as number).toLocaleString()} aUEC`;
  }

  /** Per-entry pick probability within a pool (weights are relative). */
  function entryPct(weight: number, total: number): string {
    if (total <= 0) return "—";
    return `${Math.round((weight / total) * 100)}%`;
  }

  /** Collapse a pool's blueprint entries to one per record guid (summing
   *  weights) — a pool can legitimately list the same BP twice, but for
   *  ownership tracking it's one entry. Also keeps the keyed `each` unique. */
  type PoolBp = MissionView["blueprint_rewards"][number]["blueprints"][number];
  function dedupeBlueprints(blueprints: PoolBp[]): PoolBp[] {
    const byGuid = new Map<string, PoolBp>();
    for (const b of blueprints) {
      const existing = byGuid.get(b.blueprint_record_guid);
      if (existing) existing.weight += b.weight;
      else byGuid.set(b.blueprint_record_guid, { ...b });
    }
    return [...byGuid.values()];
  }

  function formatSeconds(s: number | null): string {
    if (s == null || s <= 0) return "—";
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    if (h > 0) return m > 0 ? `${h}h ${m}m` : `${h}h`;
    if (m > 0) return `${m}m`;
    return `${Math.round(s)}s`;
  }

  const filters: { id: Filter; label: string }[] = [
    { id: "grantsbp", label: "Grants BP" },
    { id: "all", label: "All" },
    { id: "outstanding", label: "Outstanding" },
    { id: "exhausted", label: "Exhausted" },
  ];
</script>

<PageHeader title="Missions">
  {#snippet subtitle()}
    {#if loading}Loading…{:else}{missions.length} missions · {outstandingCount} with blueprints to collect{/if}
  {/snippet}
  <input
    class="search"
    type="search"
    placeholder="Search mission name…"
    bind:value={query}
    disabled={loading}
  />
</PageHeader>

{#if loading}
  <Loading />
{:else if errorMessage}
  <div class="error"><strong>Couldn't load missions.</strong><p>{errorMessage}</p></div>
{:else}
  {#if bpFilter}
    <div class="bp-banner">
      <span class="bp-banner-text">
        Showing missions that grant
        <strong>{bpFilterName ?? "the selected blueprint"}</strong>
        <span class="bp-banner-n">· {filtered.length}</span>
      </span>
      <a class="bp-clear" href="/missions" title="Clear this filter">all missions ✕</a>
    </div>
  {/if}
  <div class="filterbar">
    <!-- Primary status mode. -->
    <div class="fgroup chips">
      {#each filters as f (f.id)}
        <button class="chip" class:on={filter === f.id} onclick={() => (filter = f.id)}>
          {f.label}
          {#if f.id === "outstanding"}<span class="chip-n">{outstandingCount}</span>{/if}
        </button>
      {/each}
    </div>

    <div class="fgroup right">
      <label class="sort-ctl">
        <span class="sort-k">Sort</span>
        <select class="facet" bind:value={sortMode}>
          <option value="title">Title A–Z</option>
          <option value="payout-desc">Payout high → low</option>
          <option value="payout-asc">Payout low → high</option>
        </select>
      </label>
      <button
        class="group-toggle"
        class:on={grouped}
        onclick={() => (grouped = !grouped)}
        title="Collapse same-title variants into one row"
      >
        <span class="group-ico" aria-hidden="true">{grouped ? "▰" : "▭"}</span>
        Group
      </button>
      <!-- Refinement filters tucked into a popover. -->
      <div class="filters-wrap">
        <button class="filters-btn" class:on={filtersOpen || activeFilterCount > 0} onclick={() => (filtersOpen = !filtersOpen)}>
          Filters
          {#if activeFilterCount}<span class="filters-n">{activeFilterCount}</span>{/if}
          <span class="filters-caret" class:open={filtersOpen} aria-hidden="true">▾</span>
        </button>
        {#if filtersOpen}
          <button class="popover-backdrop" onclick={() => (filtersOpen = false)} aria-label="Close filters"></button>
          <div class="filters-popover">
            <label class="fctl">
              <span class="fctl-k">Mission type</span>
              <select class="facet" bind:value={categoryFilter}>
                <option value="">All types</option>
                {#each categoryList as c (c)}<option value={c}>{c}</option>{/each}
              </select>
            </label>
            <label class="fctl">
              <span class="fctl-k">Faction</span>
              <select class="facet" bind:value={factionFilter}>
                <option value="">All factions</option>
                {#each factionList as f (f)}<option value={f}>{f}</option>{/each}
              </select>
            </label>
            <label class="fctl">
              <span class="fctl-k">Star system</span>
              <select class="facet" bind:value={systemFilter}>
                <option value="">All systems</option>
                {#each systemList as s (s)}<option value={s}>{s}</option>{/each}
              </select>
            </label>
            {#if repRanks.length}
              <div class="fctl">
                <span class="fctl-k">Required reputation</span>
                <div class="rep-filter">
                  <select class="facet rep-sel" bind:value={repMin}>
                    <option value="">any</option>
                    {#each repRanks as r (r.i)}<option value={r.i}>{r.name}</option>{/each}
                  </select>
                  <span class="rep-dash">–</span>
                  <select class="facet rep-sel" bind:value={repMax}>
                    <option value="">any</option>
                    {#each repRanks as r (r.i)}<option value={r.i}>{r.name}</option>{/each}
                  </select>
                </div>
              </div>
            {/if}
            {#if activeFilterCount || query.trim()}
              <button class="clear-btn pop-clear" onclick={clearFilters}>Clear all filters</button>
            {/if}
          </div>
        {/if}
      </div>
      <span class="legend"><span class="pip exh"></span> all reward BPs owned</span>
    </div>
  </div>

  <section class="missions">
    {#snippet missionRow(m: MissionView)}
        {@const isOpen = expanded.has(m.mission_id)}
        {@const guids = rewardGuids(m)}
        {@const own = ownedCountOf(m)}
        {@const exh = guids.length > 0 && own === guids.length}
        <li id={`m-${m.mission_id}`} class:exhausted={exh} class:expanded={isOpen}>
          <div class="m-row">
            {#if guids.length > 0}
              <span
                class="bp-count"
                class:full={exh}
                class:some={own > 0 && !exh}
                title={`${own} of ${guids.length} reward blueprints owned`}
              >{own}/{guids.length}</span>
            {:else}
              <span class="bp-count empty" title="No blueprint rewards">·</span>
            {/if}
            <button class="m-expand" onclick={() => toggleExpanded(m.mission_id)}>
              <span class="chevron" class:open={isOpen} aria-hidden="true">▸</span>
              <span class="m-text">
                <span class="m-line title-line">
                  <span class="m-name" class:untitled={!m.title}>{missionTitle(m)}</span>
                  <span class="grow"></span>
                  <span class="auec" title="aUEC payout">{payoutText(m)}</span>
                  {#if m.once_only}<span class="badge once" title="Non-repeatable">once</span>{/if}
                  {#if m.illegal}<span class="badge illegal" title="Illegal contract">illegal</span>{/if}
                  {#if guids.length > 0}
                    <span class="badge bp" title="Awards blueprints">{guids.length} BP</span>
                  {/if}
                  {#if m.instance_count > 1}
                    <span class="badge inst" title="Collapses {m.instance_count} offerings">×{m.instance_count}</span>
                  {/if}
                </span>
                <span class="m-line sub-line">
                  {#if m.faction?.name}
                    <span class="faction" title="Faction">{m.faction.name}</span>
                  {/if}
                  {#if m.category?.name}
                    <span class="cat" title="Mission type">{m.category.name}</span>
                  {/if}
                  {#each systemsOf(m) as sys, i (`${sys}-${i}`)}
                    <span class="loc" title="Offered in {sys}">⌖ {sys}</span>
                  {/each}
                  {#if m.rep_required.length}<span class="badge rep" title="Reputation required">rep</span>{/if}
                  {#if m.chain_required.length}<span class="badge chain" title="Requires prior missions">chain</span>{/if}
                </span>
              </span>
            </button>
          </div>

          {#if isOpen}
            <div class="m-detail" class:split={!!m.description}>
              {#if m.description}
                <div class="detail-desc">
                  <!-- SC locale stores line breaks as the literal two-char `\n`
                       (the engine interprets them); convert to real newlines and
                       let CSS render them (.m-desc is white-space: pre-line). -->
                  <p class="m-desc">{m.description.replaceAll("\\n", "\n")}</p>
                </div>
              {/if}
              <div class="detail-data">

              <!-- Top facts: type · faction · payout · difficulty. -->
              <div class="meta">
                {#if m.category?.name}
                  <span class="meta-item"><span class="meta-k">Type</span> {m.category.name}</span>
                {/if}
                {#if m.faction?.name}
                  <span class="meta-item"><span class="meta-k">Faction</span> {m.faction.name}</span>
                {/if}
                <span class="meta-item"><span class="meta-k">aUEC</span> {payoutText(m)}</span>
                {#if m.payout.buy_in > 0}
                  <span class="meta-item"><span class="meta-k">Buy-in</span> {m.payout.buy_in.toLocaleString()}</span>
                {/if}
                {#if difficultyTier(m)}
                  <span class="meta-item" title="Hidden axes: mech {m.difficulty?.mechanical_skill} · mental {m.difficulty?.mental_load} · risk {m.difficulty?.risk_of_loss} · knowledge {m.difficulty?.game_knowledge}">
                    <span class="meta-k">Difficulty</span> {difficultyTier(m)}/8
                  </span>
                {/if}
              </div>

              <!-- Acceptance gates: required reputation + prerequisite chain. -->
              {#if m.rep_required.length}
                <div class="meta">
                  {#each m.rep_required as r, i (i)}
                    <span class="meta-item">
                      <span class="meta-k">{r.exclude ? "Rep block" : "Rep req"}</span>
                      {r.faction ?? "—"}{r.min_rank ? ` · ${r.min_rank}` : ""}{r.max_rank && r.max_rank !== r.min_rank ? `–${r.max_rank}` : ""}
                    </span>
                  {/each}
                </div>
              {/if}
              {#if m.chain_required.length}
                <div class="meta">
                  <div class="meta-item meta-loc">
                    <span class="meta-k">Requires</span>
                    <div class="loc-lines">
                      {#each m.chain_required as c (c.mission_id)}
                        <button class="chain-link" onclick={() => jumpToMission(c)} title="Jump to this mission">
                          ↳ {c.title ?? c.mission_id}
                        </button>
                      {/each}
                    </div>
                  </div>
                </div>
              {/if}

              <!-- Reward summary (non-blueprint axes). -->
              {#if m.scrip.length || m.reputation.length || m.item_rewards.length || m.cooldown_seconds}
                <div class="rewards">
                  {#each m.scrip as s, i (i)}
                    <span class="rw"><span class="rw-k">{s.name ?? "Scrip"}</span> {s.amount.toLocaleString()}</span>
                  {/each}
                  {#each m.reputation as r, i (i)}
                    <span class="rw" title={r.faction_guid ?? ""}>
                      <span class="rw-k">Rep</span> {r.amount != null ? (r.amount > 0 ? `+${r.amount}` : `${r.amount}`) : "varies"}
                    </span>
                  {/each}
                  {#each m.item_rewards as it, i (i)}
                    <span class="rw"><span class="rw-k">Item</span> {it.name ?? "Unknown item"}{it.amount > 1 ? ` ×${it.amount}` : ""}</span>
                  {/each}
                  {#if m.cooldown_seconds}
                    <span class="rw"><span class="rw-k">Cooldown</span> {formatSeconds(m.cooldown_seconds)}</span>
                  {/if}
                </div>
              {/if}

              <!-- Blueprint pools — the reward checkbox IS catalog ownership. -->
              {#if grantsBp(m)}
                <div class="pools">
                  {#each m.blueprint_rewards as pool, pi (pi)}
                    {@const bps = dedupeBlueprints(pool.blueprints)}
                    {@const total = bps.reduce((n, b) => n + b.weight, 0)}
                    <div class="pool">
                      <div class="pool-head">
                        <span class="pool-name">{pool.pool_name || "Blueprint pool"}</span>
                        <span class="pool-chance" title="Chance the blueprint draw happens">
                          drops {Math.round(pool.chance * 100)}%
                        </span>
                      </div>
                      {#each bps as b (b.blueprint_record_guid)}
                        {@const isOwned = owned.has(b.blueprint_record_guid)}
                        <div class="bp-reward" class:owned={isOwned}>
                          <button
                            class="own-toggle small"
                            class:on={isOwned}
                            title={isOwned ? "Blueprint owned — click to unmark" : "Mark blueprint owned"}
                            onclick={() => toggleOwned(b.blueprint_record_guid)}
                          >{isOwned ? "✓" : ""}</button>
                          <span class="bp-name">{b.name ?? "Unknown blueprint"}</span>
                          <span class="bp-weight" title="Pick probability within this pool">{entryPct(b.weight, total)}</span>
                        </div>
                      {/each}
                    </div>
                  {/each}
                </div>
              {/if}

              <!-- Where it's offered — parent locality cards, click to expand. -->
              {#if m.locations.length}
                {@const sel = selectedRegion(m)}
                <div class="loc-block">
                  <div class="loc-head">Available in</div>
                  <div class="loc-cards">
                    {#each m.locations as reg, ri (ri)}
                      {@const isSel = selectedLoc.get(m.mission_id) === ri}
                      <button
                        class="loc-card"
                        class:sel={isSel}
                        onclick={() => toggleLoc(m.mission_id, ri)}
                      >
                        <span class="loc-card-name">{reg.system}{reg.name ? ` — ${reg.name}` : ""}</span>
                        <span class="loc-card-n">{reg.places.length}</span>
                        <span class="loc-card-caret" class:open={isSel} aria-hidden="true">▸</span>
                      </button>
                    {/each}
                  </div>
                  {#if sel}
                    <div class="loc-detail">
                      {#each kindBuckets(sel.places) as bucket (bucket.label)}
                        <div class="loc-bucket">
                          <span class="loc-bucket-k">{bucket.label}</span>
                          <div class="loc-places">
                            {#each bucket.places as p (p.record_name)}
                              <span class="loc-place">{p.name ?? p.record_name}</span>
                            {/each}
                          </div>
                        </div>
                      {/each}
                    </div>
                  {/if}
                </div>
              {/if}

              <!-- Encounters — collapsible ship pools + cargo per wave. -->
              {#if m.encounters.length}
                <div class="enc-block">
                  {#each m.encounters as enc, ei (ei)}
                    <div class="enc-head">Encounters{enc.difficulty ? ` · ${enc.difficulty}` : ""}</div>
                    {#each enc.waves as w, wi (wi)}
                      <div class="wave">
                        <div class="wave-row">
                          <span class="wave-name">{w.name || "Wave"}</span>
                          {#each w.ships as slot, si (si)}
                            {@const key = `${m.mission_id}-${ei}-${wi}-${si}`}
                            <span class="wave-count">{shipRange(slot)}× ship{slot.count_max === 1 ? "" : "s"}</span>
                            {#if slot.ships.length}
                              <button class="ships-toggle" onclick={() => toggleSet(expandedShips, key)}>
                                {slot.ships.length} types
                                <span class="mini-caret" class:open={expandedShips.has(key)} aria-hidden="true">▸</span>
                              </button>
                            {/if}
                          {/each}
                          {#if w.cargo.length}<span class="wave-cargo">{w.cargo.join(" · ")}</span>{/if}
                        </div>
                        {#each w.ships as slot, si (si)}
                          {@const key = `${m.mission_id}-${ei}-${wi}-${si}`}
                          {#if expandedShips.has(key)}
                            <div class="ship-chips">
                              {#each slot.ships as s (s)}<span class="ship-chip">{s}</span>{/each}
                            </div>
                          {/if}
                        {/each}
                      </div>
                    {/each}
                  {/each}
                </div>
              {/if}

              {#if m.placeholders.length}
                <div class="meta">
                  <span class="meta-item"><span class="meta-k">Variables</span> {m.placeholders.map((p) => `~${p}`).join(" ")}</span>
                </div>
              {/if}
              </div>
            </div>
          {/if}
        </li>
    {/snippet}

    <ul>
      {#if grouped}
        {#each families.slice(0, visibleCount) as fam (fam.id)}
          {#if fam.members.length === 1}
            {@render missionRow(fam.members[0])}
          {:else}
            {@const fopen = expandedFamilies.has(fam.id)}
            <li class="fam-group" class:exhausted={fam.exhausted} class:expanded={fopen}>
              <div class="m-row">
                {#if fam.bpGuids.length > 0}
                  <span
                    class="bp-count"
                    class:full={fam.exhausted}
                    class:some={fam.ownedCount > 0 && !fam.exhausted}
                    title={`${fam.ownedCount} of ${fam.bpGuids.length} reward blueprints owned across ${fam.members.length} variants`}
                  >{fam.ownedCount}/{fam.bpGuids.length}</span>
                {:else}
                  <span class="bp-count empty" title="No blueprint rewards">·</span>
                {/if}
                <button class="m-expand" onclick={() => toggleSet(expandedFamilies, fam.id)}>
                  <span class="chevron" class:open={fopen} aria-hidden="true">▸</span>
                  <span class="m-text">
                    <span class="m-line title-line">
                      <span class="m-name">{fam.title}</span>
                      <span class="grow"></span>
                      <span class="auec" title="aUEC payout range across variants">{payoutRange(fam)}</span>
                      <span class="badge fam-n" title="{fam.members.length} variants collapsed here">×{fam.members.length}</span>
                      {#if fam.bpGuids.length > 0}
                        <span class="badge bp" title="Distinct reward blueprints across all variants">{fam.bpGuids.length} BP</span>
                      {/if}
                    </span>
                    <span class="m-line sub-line">
                      {#if fam.faction}<span class="faction" title="Faction">{fam.faction}</span>{/if}
                      {#if fam.category}<span class="cat" title="Mission type">{fam.category}</span>{/if}
                      {#each fam.systems as sys, i (`${sys}-${i}`)}<span class="loc" title="Offered in {sys}">⌖ {sys}</span>{/each}
                      {#if fam.rep}<span class="badge rep" title="Reputation required">rep</span>{/if}
                      {#if !fam.exhausted && fam.collectable > 0}
                        <span class="badge collect" title="Variants that still grant blueprints you don't own">{fam.collectable} to collect</span>
                      {/if}
                    </span>
                  </span>
                </button>
              </div>
              {#if fopen}
                <ul class="fam-members">
                  {#each fam.members as m (m.mission_id)}
                    {@render missionRow(m)}
                  {/each}
                </ul>
              {/if}
            </li>
          {/if}
        {/each}
      {:else}
        {#each sorted.slice(0, visibleCount) as m (m.mission_id)}
          {@render missionRow(m)}
        {/each}
      {/if}
    </ul>
    {#if renderCount === 0}
      <p class="status">No missions match.</p>
    {:else if visibleCount < renderCount}
      <button class="show-more" onclick={() => (visibleCount += PAGE)}>
        Show more · {renderCount - visibleCount} of {renderCount} remaining
      </button>
    {/if}
  </section>
{/if}

<style>
  .search {
    margin-left: auto;
    width: 280px;
    padding: 0.5rem 0.8rem;
    background: var(--panel-2);
    border: 1px solid var(--line);
    border-radius: 8px;
    outline: none;
  }
  .search:focus { border-color: var(--ember); }

  .status, .error { padding: 1rem 1.6rem; color: var(--muted); }
  .error strong { color: var(--bad); }

  /* Cross-link banner shown when arriving from the wishlist (?bp=…). */
  .bp-banner {
    display: flex;
    align-items: center;
    gap: 0.8rem;
    margin: 0.85rem 1.6rem 0;
    padding: 0.5rem 0.85rem;
    border: 1px solid var(--ember-dim);
    border-radius: 8px;
    background: var(--ember-glow);
    font-size: 0.82rem;
  }
  .bp-banner-text { color: var(--muted); }
  .bp-banner-text strong { color: var(--ember); }
  .bp-banner-n { color: var(--faint); font-variant-numeric: tabular-nums; }
  .bp-clear {
    margin-left: auto;
    color: var(--muted);
    text-decoration: none;
    font-size: 0.78rem;
    white-space: nowrap;
    padding: 0.2rem 0.5rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    transition: all 90ms;
  }
  .bp-clear:hover { color: var(--text); border-color: var(--ember-dim); }

  /* Filter bar: spaced groups (status · facets · rep · clear) that wrap. */
  .filterbar { display: flex; flex-wrap: wrap; align-items: center; gap: 0.5rem 1.2rem; padding: 0.85rem 1.6rem; }
  .fgroup { display: flex; flex-wrap: wrap; align-items: center; gap: 0.45rem; }
  .fgroup.right { margin-left: auto; gap: 0.8rem; }
  .clear-btn {
    background: transparent;
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--muted);
    font-size: 0.74rem;
    padding: 0.25rem 0.6rem;
    cursor: pointer;
    white-space: nowrap;
    transition: all 90ms;
  }
  .clear-btn:hover { color: var(--text); border-color: var(--ember-dim); }

  /* "Filters ▾" popover holding the refinement filters. */
  .filters-wrap { position: relative; }
  .filters-btn {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.32rem 0.75rem;
    background: var(--panel-2);
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--muted);
    font-size: 0.8rem;
    cursor: pointer;
    transition: all 90ms;
  }
  .filters-btn:hover { color: var(--text); }
  .filters-btn.on { background: var(--ember-glow); border-color: var(--ember-dim); color: var(--ember); }
  .filters-n {
    font-size: 0.66rem;
    font-variant-numeric: tabular-nums;
    background: var(--ember);
    color: var(--on-ember);
    border-radius: 999px;
    min-width: 1.1rem;
    text-align: center;
    padding: 0.02rem 0.3rem;
  }
  .filters-caret { font-size: 0.6rem; color: var(--faint); transition: transform 120ms ease-out; }
  .filters-caret.open { transform: rotate(180deg); color: var(--ember); }
  .popover-backdrop { position: fixed; inset: 0; z-index: 40; background: transparent; border: none; cursor: default; }
  .filters-popover {
    position: absolute;
    top: calc(100% + 0.4rem);
    right: 0;
    z-index: 41;
    width: 17rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
    padding: 0.85rem;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 10px;
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.4);
  }
  .fctl { display: flex; flex-direction: column; gap: 0.25rem; }
  .fctl-k { font-size: 0.62rem; text-transform: uppercase; letter-spacing: 0.06em; color: var(--faint); }
  .fctl > .facet { max-width: none; width: 100%; }
  .pop-clear { align-self: flex-start; margin-top: 0.1rem; }
  /* Reputation tier-range — two selectors side by side inside the popover. */
  .rep-filter { display: flex; align-items: center; gap: 0.4rem; }
  .rep-sel { max-width: none; flex: 1; min-width: 0; }
  .rep-dash { color: var(--faint); }
  /* Prerequisite-mission jump link. */
  .chain-link {
    display: inline-block;
    background: none;
    border: none;
    padding: 0;
    color: var(--ember);
    font-size: inherit;
    text-align: left;
    cursor: pointer;
  }
  .chain-link:hover { text-decoration: underline; }
  .chips { display: flex; gap: 0.5rem; }
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
  .chip:hover { color: var(--text); }
  .chip.on { background: var(--ember-glow); border-color: var(--ember-dim); color: var(--ember); }
  .chip-n { font-size: 0.68rem; opacity: 0.8; font-variant-numeric: tabular-nums; }
  .legend { display: inline-flex; align-items: center; gap: 0.35rem; font-size: 0.72rem; color: var(--faint); white-space: nowrap; }
  /* List sort control — inline label + select in the filter bar. */
  .sort-ctl { display: inline-flex; align-items: center; gap: 0.4rem; }
  .sort-k { font-size: 0.62rem; text-transform: uppercase; letter-spacing: 0.06em; color: var(--faint); }
  /* Incremental "show more" — reveals the next page of rows. */
  .show-more {
    display: block;
    margin: 0.9rem auto 0;
    padding: 0.45rem 1.1rem;
    background: var(--panel-2);
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--muted);
    font-size: 0.8rem;
    cursor: pointer;
    transition: all 90ms;
  }
  .show-more:hover { color: var(--text); border-color: var(--ember); }
  /* Group toggle — same pill shape as the Filters button. */
  .group-toggle {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.32rem 0.75rem;
    background: var(--panel-2);
    border: 1px solid var(--line);
    border-radius: 999px;
    color: var(--muted);
    font-size: 0.8rem;
    cursor: pointer;
    transition: all 90ms;
  }
  .group-toggle:hover { color: var(--text); }
  .group-toggle.on { color: var(--ember); border-color: var(--ember); }
  .group-ico { font-size: 0.62rem; }
  /* Mission family — the collapsed group row reuses .m-row / li styling; its
     expanded variants nest under an indented, ruled sub-list. */
  .fam-members {
    list-style: none;
    margin: 0 0 0.2rem 1.15rem;
    padding: 0 0 0 0.45rem;
    border-left: 2px solid var(--line);
  }
  .badge.fam-n { background: var(--panel-2); color: var(--muted); }
  .badge.collect { background: var(--ember-glow); color: var(--ember); border-color: var(--ember); }
  .pip { width: 0.6rem; height: 0.6rem; border-radius: 2px; display: inline-block; }
  .pip.exh { background: var(--ember); }

  .missions { flex: 1; overflow-y: auto; padding: 0 1.6rem 2rem; }
  ul { list-style: none; margin: 0; padding: 0; }
  li { border-radius: 8px; border: 1px solid transparent; }
  li:hover { background: var(--panel); }
  li.exhausted { background: linear-gradient(90deg, var(--ember-glow), transparent 60%); }
  li.expanded { background: var(--panel); border-color: var(--line); }

  .m-row { display: flex; align-items: center; gap: 0.8rem; padding: 0.5rem 0.6rem; }
  /* Derived "owned reward BPs" count — full ember when exhausted. */
  .bp-count {
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
  }
  .bp-count.some { color: var(--ember); border-color: var(--ember-dim); }
  .bp-count.full { color: var(--on-ember); background: var(--ember); border-color: var(--ember); font-weight: 700; }
  .bp-count.empty { color: var(--faint); background: transparent; border-color: transparent; }

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
  .own-toggle:hover { border-color: var(--ember-dim); }
  .own-toggle.on { background: var(--ember); border-color: var(--ember); color: var(--on-ember); font-weight: 700; }
  .own-toggle.small { width: 1.2rem; height: 1.2rem; font-size: 0.7rem; }

  .m-expand {
    flex: 1;
    display: flex;
    align-items: flex-start;
    gap: 0.55rem;
    padding: 0.1rem;
    background: transparent;
    border: none;
    color: inherit;
    text-align: left;
    cursor: pointer;
    min-width: 0;
  }
  .chevron { width: 0.9rem; margin-top: 0.18rem; flex: 0 0 auto; color: var(--faint); font-size: 0.75rem; transition: transform 120ms ease-out; }
  .chevron.open { transform: rotate(90deg); color: var(--ember); }

  /* Two-line entry: title line over a faction/meta sub-line (in-game style). */
  .m-text { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 0.1rem; }
  .m-line { display: flex; align-items: center; gap: 0.5rem; min-width: 0; }
  .grow { flex: 1 1 auto; }
  .sub-line { gap: 0.6rem; }
  .m-name { flex: 0 1 auto; font-size: 0.9rem; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .m-name.untitled { color: var(--faint); font-family: ui-monospace, Consolas, monospace; font-size: 0.82rem; }

  .badge {
    flex: 0 0 auto;
    font-size: 0.62rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    padding: 0.05rem 0.4rem;
    border-radius: 4px;
    border: 1px solid var(--line);
    color: var(--muted);
  }
  .badge.once { color: var(--ember); border-color: var(--ember-dim); }
  .badge.illegal { color: var(--bad); border-color: var(--bad); }
  .badge.bp { color: var(--ember); background: var(--ember-glow); border-color: var(--ember-dim); font-variant-numeric: tabular-nums; }
  .badge.inst { font-variant-numeric: tabular-nums; }
  /* Location hint on the row — distinct from the grey structural badges. */
  .loc {
    flex: 0 0 auto;
    font-size: 0.7rem;
    color: var(--ember);
    background: var(--ember-glow);
    border: 1px solid var(--ember-dim);
    border-radius: 999px;
    padding: 0.05rem 0.5rem;
    max-width: 16ch;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Indent the detail so it lines up under the title text (past the
     bp-count chip + chevron), not under the chip. */
  .m-detail { padding: 0.2rem 0.8rem 0.8rem; padding-left: 3.95rem; }
  /* Split layout: description (tall, narrow — intentional line breaks) on the
     left, the structured data on the right. Stacks on narrow windows. */
  .m-detail.split {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1.3fr);
    gap: 0 1.6rem;
    align-items: start;
  }
  @media (max-width: 1080px) {
    .m-detail.split { grid-template-columns: 1fr; gap: 0; }
  }
  .detail-desc { min-width: 0; }
  .detail-data { min-width: 0; }
  .meta { display: flex; flex-wrap: wrap; align-items: flex-start; gap: 0.2rem 1.1rem; margin-bottom: 0.5rem; }
  .meta-item { font-size: 0.78rem; color: var(--text); }
  .meta-k { color: var(--faint); text-transform: uppercase; letter-spacing: 0.04em; font-size: 0.64rem; margin-right: 0.35rem; }
  /* Location: label beside a stack of one region per line. */
  .meta-loc { display: flex; align-items: baseline; }
  .loc-lines { display: flex; flex-direction: column; gap: 0.1rem; }
  .m-desc { margin: 0 0 0.6rem; font-size: 0.82rem; color: var(--muted); max-width: 70ch; white-space: pre-line; }

  .rewards { display: flex; flex-wrap: wrap; gap: 0.4rem; margin-bottom: 0.6rem; }
  .rw {
    font-size: 0.74rem;
    color: var(--text);
    background: var(--panel-2);
    border: 1px solid var(--line);
    border-radius: 999px;
    padding: 0.1rem 0.55rem;
    font-variant-numeric: tabular-nums;
  }
  .rw-k { color: var(--faint); text-transform: uppercase; letter-spacing: 0.04em; font-size: 0.64rem; margin-right: 0.3rem; }

  .pools { display: flex; flex-direction: column; gap: 0.6rem; }
  .pool { border-top: 1px dashed var(--line); padding-top: 0.4rem; }
  .pool-head { display: flex; align-items: baseline; gap: 0.6rem; margin-bottom: 0.25rem; }
  .pool-name { font-size: 0.78rem; font-weight: 600; color: var(--muted); }
  .pool-chance { font-size: 0.68rem; color: var(--faint); font-variant-numeric: tabular-nums; }
  .bp-reward { display: flex; align-items: center; gap: 0.6rem; padding: 0.2rem 0; }
  .bp-reward.owned .bp-name { color: var(--ember); }
  .bp-name { flex: 1; font-size: 0.85rem; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .bp-weight { flex: 0 0 auto; font-size: 0.7rem; color: var(--faint); font-variant-numeric: tabular-nums; }

  /* ── Missions-sweep additions ──────────────────────────────────────── */
  .facet {
    padding: 0.3rem 0.5rem;
    background: var(--panel-2);
    border: 1px solid var(--line);
    border-radius: 8px;
    color: var(--text);
    font-size: 0.78rem;
    max-width: 12rem;
  }
  .facet:focus { border-color: var(--ember); outline: none; }

  /* Category: a compact card leading the faction line. */
  .cat {
    flex: 0 0 auto;
    font-size: 0.66rem;
    color: var(--muted);
    background: var(--panel-2);
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 0.05rem 0.45rem;
    white-space: nowrap;
  }
  /* Faction: the giver subtitle under the title, in-game style. */
  .faction {
    flex: 0 1 auto;
    font-size: 0.78rem;
    color: var(--muted);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .badge.rep { color: var(--ember); border-color: var(--ember-dim); }
  .badge.chain { color: var(--muted); border-color: var(--line); }
  .auec {
    flex: 0 0 auto;
    font-size: 0.72rem;
    color: var(--text);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  /* Encounters — per-wave row with a collapsible ship-candidate pool. */
  .enc-block { margin-bottom: 0.7rem; }
  .enc-head { font-size: 0.62rem; text-transform: uppercase; letter-spacing: 0.08em; color: var(--faint); margin-bottom: 0.3rem; }
  .wave { padding: 0.15rem 0; }
  .wave-row { display: flex; flex-wrap: wrap; align-items: center; gap: 0.5rem; }
  .wave-name { font-size: 0.78rem; color: var(--text); font-weight: 600; min-width: 5rem; }
  .wave-count {
    font-size: 0.74rem;
    color: var(--text);
    background: var(--panel-2);
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 0.05rem 0.45rem;
    font-variant-numeric: tabular-nums;
  }
  .ships-toggle {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    background: transparent;
    border: none;
    color: var(--muted);
    font-size: 0.74rem;
    cursor: pointer;
    padding: 0;
  }
  .ships-toggle:hover { color: var(--ember); }
  .mini-caret { font-size: 0.6rem; color: var(--faint); transition: transform 120ms ease-out; }
  .mini-caret.open { transform: rotate(90deg); color: var(--ember); }
  .wave-cargo { font-size: 0.7rem; color: var(--faint); margin-left: auto; }
  .ship-chips { display: flex; flex-wrap: wrap; gap: 0.3rem; padding: 0.35rem 0 0.2rem 5.5rem; }
  .ship-chip {
    font-size: 0.72rem;
    color: var(--muted);
    background: var(--panel-2);
    border: 1px solid var(--line);
    border-radius: 5px;
    padding: 0.02rem 0.4rem;
    white-space: nowrap;
  }

  /* "Available in" — parent locality cards + an expandable place breakdown. */
  .loc-block { margin-bottom: 0.7rem; }
  .loc-head { font-size: 0.62rem; text-transform: uppercase; letter-spacing: 0.08em; color: var(--faint); margin-bottom: 0.4rem; }
  .loc-cards { display: flex; flex-wrap: wrap; gap: 0.4rem; margin-bottom: 0.5rem; }
  .loc-card {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.3rem 0.6rem;
    background: var(--panel-2);
    border: 1px solid var(--line);
    border-radius: 8px;
    color: var(--text);
    font-size: 0.78rem;
    cursor: pointer;
    transition: all 90ms;
  }
  .loc-card:hover { border-color: var(--ember-dim); }
  .loc-card.sel { background: var(--ember-glow); border-color: var(--ember); color: var(--ember); }
  .loc-card-n {
    font-size: 0.66rem;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
    background: var(--bg);
    border-radius: 999px;
    padding: 0.02rem 0.4rem;
  }
  .loc-card.sel .loc-card-n { color: var(--ember); background: var(--panel); }
  .loc-card-caret { font-size: 0.6rem; color: var(--faint); transition: transform 120ms ease-out; }
  .loc-card-caret.open { transform: rotate(90deg); color: var(--ember); }

  .loc-detail {
    border: 1px solid var(--line);
    border-radius: 8px;
    background: var(--panel-2);
    padding: 0.6rem 0.8rem;
  }
  .loc-bucket { margin-bottom: 0.5rem; }
  .loc-bucket:last-child { margin-bottom: 0; }
  .loc-bucket-k { display: block; font-size: 0.6rem; text-transform: uppercase; letter-spacing: 0.07em; color: var(--faint); margin-bottom: 0.25rem; }
  .loc-places { display: grid; grid-template-columns: repeat(auto-fill, minmax(9rem, 1fr)); gap: 0.15rem 0.8rem; }
  .loc-place { font-size: 0.8rem; color: var(--text); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .loc-place::before { content: "· "; color: var(--faint); }
</style>
