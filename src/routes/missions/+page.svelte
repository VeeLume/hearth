<script lang="ts">
  import { onMount } from "svelte";
  import { page } from "$app/state";
  import { SvelteSet } from "svelte/reactivity";
  import { commands, type MissionView } from "$lib/bindings";
  import Loading from "$lib/Loading.svelte";
  import { data, owned, ensureMissions, ensureOwnership } from "$lib/data.svelte";

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

  type Filter = "grantsbp" | "all" | "outstanding" | "exhausted";
  let filter = $state<Filter>("grantsbp");

  const RENDER_CAP = 500;

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

  /** Distinct star systems from region labels ("Pyro: Bloom" → "Pyro"). A
   *  compact location hint for the row; the full regions show on expand. */
  function systemsOf(regions: string[]): string[] {
    const sys = new Set<string>();
    for (const r of regions) {
      const head = r.split(":")[0].split(" (")[0].trim();
      for (const s of head.split(" + ")) if (s.trim()) sys.add(s.trim());
    }
    return [...sys];
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

  /** Toggle catalog ownership of a reward blueprint (optimistic). */
  async function toggleOwned(guid: string) {
    const was = owned.has(guid);
    if (was) owned.delete(guid);
    else owned.add(guid);
    const res = await commands.toggleOwned(guid);
    if (res.status === "ok") {
      if (res.data) owned.add(guid);
      else owned.delete(guid);
    } else {
      if (was) owned.add(guid);
      else owned.delete(guid);
      errorMessage = `${res.error.kind}: ${res.error.message}`;
    }
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
      if (q) {
        const t = missionTitle(m).toLowerCase();
        if (!(t.includes(q) || m.mission_id.toLowerCase().includes(q))) return false;
      }
      return true;
    });
  });

  /** Per-entry pick probability within a pool (weights are relative). */
  function entryPct(weight: number, total: number): string {
    if (total <= 0) return "—";
    return `${Math.round((weight / total) * 100)}%`;
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

<header class="topbar">
  <div class="page-title">
    <h1>Missions</h1>
    <span class="subtitle">
      {#if loading}Loading…{:else}{missions.length} missions · {outstandingCount} with blueprints to collect{/if}
    </span>
  </div>
  <input
    class="search"
    type="search"
    placeholder="Search mission name…"
    bind:value={query}
    disabled={loading}
  />
</header>

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
    <div class="chips">
      {#each filters as f (f.id)}
        <button class="chip" class:on={filter === f.id} onclick={() => (filter = f.id)}>
          {f.label}
          {#if f.id === "outstanding"}<span class="chip-n">{outstandingCount}</span>{/if}
        </button>
      {/each}
    </div>
    <div class="legend">
      <span class="legend-item"><span class="pip exh"></span> all reward BPs owned</span>
    </div>
  </div>

  <section class="missions">
    <ul>
      {#each filtered.slice(0, RENDER_CAP) as m (m.mission_id)}
        {@const isOpen = expanded.has(m.mission_id)}
        {@const guids = rewardGuids(m)}
        {@const own = ownedCountOf(m)}
        {@const exh = guids.length > 0 && own === guids.length}
        <li class:exhausted={exh} class:expanded={isOpen}>
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
              <span class="m-name" class:untitled={!m.title}>{missionTitle(m)}</span>
              {#if m.regions.length}
                {@const systems = systemsOf(m.regions)}
                {#each (systems.length ? systems : [m.regions[0]]) as sys, i (`${sys}-${i}`)}
                  <span class="loc" title={m.regions.join("\n")}>⌖ {sys}</span>
                {/each}
              {/if}
              {#if m.once_only}<span class="badge once" title="Non-repeatable">once</span>{/if}
              {#if m.illegal}<span class="badge illegal" title="Illegal contract">illegal</span>{/if}
              {#if guids.length > 0}
                <span class="badge bp" title="Awards blueprints">{guids.length} BP</span>
              {/if}
              {#if m.instance_count > 1}
                <span class="badge inst" title="Offered at {m.instance_count} localities">×{m.instance_count}</span>
              {/if}
            </button>
          </div>

          {#if isOpen}
            <div class="m-detail">
              {#if m.description}
                <!-- SC locale stores line breaks as the literal two-char `\n`
                     (the engine interprets them); convert to real newlines and
                     let CSS render them (.m-desc is white-space: pre-line). -->
                <p class="m-desc">{m.description.replaceAll("\\n", "\n")}</p>
              {/if}

              <!-- Where it's offered + encounter banner. -->
              {#if m.regions.length || m.encounter_summary}
                <div class="meta">
                  {#if m.regions.length}
                    <div class="meta-item meta-loc">
                      <span class="meta-k">Location</span>
                      <div class="loc-lines">
                        {#each m.regions as r, i (`${r}-${i}`)}<span>{r}</span>{/each}
                      </div>
                    </div>
                  {/if}
                  {#if m.encounter_summary}
                    <span class="meta-item"><span class="meta-k">Encounters</span> {m.encounter_summary}</span>
                  {/if}
                </div>
              {/if}

              <!-- Reward summary (non-blueprint axes). -->
              <div class="rewards">
                {#if m.uec_fixed != null}
                  <span class="rw"><span class="rw-k">aUEC</span> {m.uec_fixed.toLocaleString()}</span>
                {:else if m.uec_calculated}
                  <span class="rw"><span class="rw-k">aUEC</span> varies</span>
                {/if}
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

              <!-- Blueprint pools — the reward checkbox IS catalog ownership. -->
              {#if grantsBp(m)}
                <div class="pools">
                  {#each m.blueprint_rewards as pool, pi (pi)}
                    {@const total = pool.blueprints.reduce((n, b) => n + b.weight, 0)}
                    <div class="pool">
                      <div class="pool-head">
                        <span class="pool-name">{pool.pool_name || "Blueprint pool"}</span>
                        <span class="pool-chance" title="Chance the blueprint draw happens">
                          drops {Math.round(pool.chance * 100)}%
                        </span>
                      </div>
                      {#each pool.blueprints as b (b.blueprint_record_guid)}
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
            </div>
          {/if}
        </li>
      {/each}
    </ul>
    {#if filtered.length === 0}
      <p class="status">No missions match.</p>
    {:else if filtered.length > RENDER_CAP}
      <p class="status">Showing {RENDER_CAP} of {filtered.length} — refine the search or filter.</p>
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
  .page-title { display: flex; flex-direction: column; gap: 0.15rem; }
  h1 { margin: 0; font-size: 1.4rem; letter-spacing: -0.02em; }
  .subtitle { font-size: 0.78rem; color: var(--muted); font-variant-numeric: tabular-nums; }
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

  .filterbar { display: flex; align-items: center; gap: 0.5rem; padding: 0.85rem 1.6rem; }
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
  .legend { margin-left: auto; display: flex; gap: 0.9rem; font-size: 0.72rem; color: var(--faint); }
  .legend-item { display: inline-flex; align-items: center; gap: 0.35rem; }
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
  .bp-count.full { color: #1a1209; background: var(--ember); border-color: var(--ember); font-weight: 700; }
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
  .own-toggle.on { background: var(--ember); border-color: var(--ember); color: #1a1209; font-weight: 700; }
  .own-toggle.small { width: 1.2rem; height: 1.2rem; font-size: 0.7rem; }

  .m-expand {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 0.55rem;
    padding: 0.1rem;
    background: transparent;
    border: none;
    color: inherit;
    text-align: left;
    cursor: pointer;
    min-width: 0;
  }
  .chevron { width: 0.9rem; flex: 0 0 auto; color: var(--faint); font-size: 0.75rem; transition: transform 120ms ease-out; }
  .chevron.open { transform: rotate(90deg); color: var(--ember); }
  .m-name { flex: 1; font-size: 0.9rem; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
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

  .m-detail { padding: 0.2rem 0.8rem 0.8rem 2.8rem; }
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
</style>
