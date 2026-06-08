<script lang="ts">
  import { onMount } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { page } from "$app/state";
  import type { BpView, CraftPlanEntry, Recipe } from "$lib/ipc";
  import { type Craftable, collapseCraftables, nameOf, formatScu } from "$lib/domain/catalog";
  import { categoryFor } from "$lib/domain/categories";
  import { coverageFor } from "$lib/domain/inventory";
  import { buildPlanLedger, BASE_QUALITY, type PlanEntryResult } from "$lib/domain/plan";
  import {
    data,
    owned,
    wishSet,
    ensureBlueprints,
    ensureOwnership,
    ensureInventory,
    ensureCraftPlan,
    ensureCraftProjects,
    addPlanEntry,
    updatePlanEntry,
    removePlanEntry,
    reorderPlan,
    createProject,
    updateProject,
    deleteProject,
    setProjectActive,
    reorderProjects,
  } from "$lib/state/data.svelte";
  import PageHeader from "$lib/components/PageHeader.svelte";
  import Loading from "$lib/components/Loading.svelte";
  import RecipeDetail from "$lib/components/recipe/RecipeDetail.svelte";
  import { persistentScroll } from "$lib/actions/scroll";

  // The crafting planner: a plan of intended crafts, grouped into projects you
  // order by hand, with a shared-pool reservation ledger across the whole plan.

  let loading = $state(!(data.blueprintsReady && data.craftPlanReady));
  let errorMessage = $state<string | null>(null);
  const keepScroll = persistentScroll();

  onMount(async () => {
    const [bpErr] = await Promise.all([
      ensureBlueprints(),
      ensureOwnership(),
      ensureInventory(),
      ensureCraftPlan(),
      ensureCraftProjects(),
    ]);
    if (bpErr) errorMessage = bpErr;
    loading = false;
  });

  function err(e: string | null) {
    if (e) errorMessage = e;
  }

  // Blueprint lookup by record guid → recipe / name / category for plan rows.
  const bpByGuid = $derived.by(() => {
    const m = new Map<string, BpView>();
    for (const b of data.blueprints) m.set(b.blueprint_record_guid, b);
    return m;
  });
  function recipeOf(guid: string): Recipe | null {
    return bpByGuid.get(guid)?.recipe ?? null;
  }

  const ledger = $derived(
    buildPlanLedger(data.craftPlan, data.craftProjects, recipeOf, data.inventoryByCrc),
  );

  // Plan grouped by project (projects in manual order, then Unsorted), each
  // group's entries sorted by their manual sort_key.
  const groups = $derived.by(() => {
    const byProject = new Map<string | null, PlanEntryResult[]>();
    for (const r of ledger.entries) {
      const k = r.entry.project_id ?? null;
      const arr = byProject.get(k);
      if (arr) arr.push(r);
      else byProject.set(k, [r]);
    }
    const sortEntries = (xs: PlanEntryResult[]) =>
      [...xs].sort(
        (a, b) => a.entry.sort_key.localeCompare(b.entry.sort_key) || a.entry.id.localeCompare(b.entry.id),
      );
    const out: {
      project: { id: string; name: string; active: boolean } | null;
      entries: PlanEntryResult[];
    }[] = [];
    for (const p of [...data.craftProjects].sort((a, b) => a.sort_key.localeCompare(b.sort_key)))
      out.push({
        project: { id: p.id, name: p.name, active: p.active },
        entries: sortEntries(byProject.get(p.id) ?? []),
      });
    const unsorted = byProject.get(null) ?? [];
    if (unsorted.length > 0) out.push({ project: null, entries: sortEntries(unsorted) });
    return out;
  });

  const inactiveCount = $derived(data.craftProjects.filter((p) => !p.active).length);

  // The actionable "still need" digest: every material still short at the
  // required quality (quantity and quality gaps unified — `short` is the unmet
  // need after quality-aware reservation).
  const stillNeed = $derived(ledger.materials.filter((m) => m.short > 0));

  const craftables = $derived(collapseCraftables(data.blueprints));
  const planEmpty = $derived(data.craftPlan.length === 0 && data.craftProjects.length === 0);

  // Plan vs the "Craftable now" discovery view (URL-driven, like ?bp).
  const view = $derived(page.url.searchParams.get("view") === "craftable" ? "craftable" : "plan");
  const hasInventory = $derived(data.inventory.length > 0);

  // "Craftable now": owned recipes your *full* inventory covers right now,
  // independent of the plan's reservations. The discovery overview that the
  // planner rework dropped.
  function craftableOwned(c: Craftable): boolean {
    return c.bpGuids.some((g) => owned.has(g));
  }
  function cov(c: Craftable) {
    return coverageFor(c.rep.recipe, data.inventoryByCrc);
  }
  function covRatio(c: Craftable): number {
    const x = cov(c);
    if (!x || x.ingredients.length === 0) return 0;
    return x.ingredients.filter((i) => i.satisfied).length / x.ingredients.length;
  }
  const readyNow = $derived(
    hasInventory
      ? craftables
          .filter((c) => c.rep.recipe && craftableOwned(c) && cov(c)?.craftable)
          .sort((a, b) => nameOf(a.rep).localeCompare(nameOf(b.rep)))
      : [],
  );
  const almostNow = $derived(
    hasInventory
      ? craftables
          .filter((c) => {
            if (!c.rep.recipe || !craftableOwned(c)) return false;
            const x = cov(c);
            return !!x && x.anyTracked && !x.craftable;
          })
          .sort((a, b) => covRatio(b) - covRatio(a) || nameOf(a.rep).localeCompare(nameOf(b.rep)))
      : [],
  );

  function nameOfGuid(guid: string): string {
    const bp = bpByGuid.get(guid);
    return bp ? nameOf(bp) : guid;
  }
  function categoryLabel(guid: string): string {
    const bp = bpByGuid.get(guid);
    if (!bp) return "";
    const k = categoryFor(bp.category_raw, bp.item_type, bp.item_sub_type);
    return k.sub ? `${k.main} · ${k.sub}` : k.main;
  }
  function fmtAmt(n: number, kind: "resource" | "item"): string {
    return kind === "item" ? `×${Math.round(n)}` : `${formatScu(n)} SCU`;
  }
  function readyCount(entries: PlanEntryResult[]): number {
    return entries.filter((e) => e.readiness === "ready").length;
  }
  function groupKey(projectId: string | null): string {
    return projectId ?? "unsorted";
  }

  // Per-row status. Short materials are marked quality vs quantity *individually*
  // (a "Q" badge on the quality ones), not lumped — so a quantity-short material
  // never inherits a sibling's quality flag. Full breakdown in the tooltip.
  type RowStatus =
    | { state: "ready" }
    | { state: "parked" }
    | { state: "short"; partial: boolean; items: { name: string; quality: boolean }[] }
    | null;
  function rowStatus(r: PlanEntryResult): RowStatus {
    if (r.readiness === "untracked") return null;
    if (r.readiness === "excluded") return { state: "parked" };
    if (r.readiness === "ready") return { state: "ready" };
    const items = r.ingredients
      .filter((i) => i.short > 0)
      .map((i) => ({ name: i.name ?? "material", quality: i.shortKind === "quality" }));
    return { state: "short", partial: r.readiness === "partial", items };
  }

  // Per-entry inline expansion — the material breakdown, in place of a tooltip.
  const expanded = new SvelteSet<string>();
  function toggleExpand(id: string) {
    if (expanded.has(id)) expanded.delete(id);
    else expanded.add(id);
  }

  // ── Entry edits ───────────────────────────────────────────────────────────
  async function patch(e: CraftPlanEntry, change: Partial<CraftPlanEntry>) {
    err(await updatePlanEntry({ ...e, ...change }));
  }
  function setQuality(e: CraftPlanEntry, raw: string) {
    const t = raw.trim();
    let q: number | null = t === "" ? null : Math.round(Number(t));
    if (q != null && Number.isNaN(q)) q = null;
    if (q != null) q = Math.max(0, Math.min(1000, q));
    patch(e, { target_quality: q });
  }

  // ── Add picker ────────────────────────────────────────────────────────────
  let picker = $state<{ open: boolean; query: string; target: string | null }>({
    open: false,
    query: "",
    target: null,
  });
  function openPicker(target: string | null) {
    picker = { open: true, query: "", target };
  }
  const inPlan = $derived(new Set(data.craftPlan.map((e) => e.blueprint_guid)));
  const pickerResults = $derived.by(() => {
    if (!picker.open) return [];
    const q = picker.query.trim().toLowerCase();
    const base = craftables.filter((c) => c.rep.recipe && !c.bpGuids.some((g) => inPlan.has(g)));
    const pool = q
      ? base.filter((c) => nameOf(c.rep).toLowerCase().includes(q))
      : // Empty query → lead with the ♡ wishlist as suggestions.
        base.filter((c) => c.bpGuids.some((g) => wishSet("item").has(g)));
    return pool.sort((a, b) => nameOf(a.rep).localeCompare(nameOf(b.rep))).slice(0, 40);
  });
  async function pick(guid: string) {
    err(await addPlanEntry(guid, picker.target));
  }

  // ── Projects ──────────────────────────────────────────────────────────────
  let newProjectName = $state("");
  let showNewProject = $state(false);
  async function addProject() {
    const name = newProjectName.trim();
    if (!name) return;
    const { error } = await createProject(name);
    err(error);
    newProjectName = "";
    showNewProject = false;
  }
  let editingProject = $state<string | null>(null);
  let editName = $state("");
  function startRename(id: string, name: string) {
    editingProject = id;
    editName = name;
  }
  async function commitRename(id: string) {
    if (editName.trim()) err(await updateProject(id, editName.trim(), null));
    editingProject = null;
  }
  async function removeProject(id: string, name: string) {
    if (!confirm(`Delete project “${name}”? Its planned crafts move to Unsorted.`)) return;
    err(await deleteProject(id));
  }

  // ── Drag reorder ──────────────────────────────────────────────────────────
  // `drop*` tracks the row the cursor is over + which side it'll land on, so the
  // UI can draw an insertion line where the drop will happen.
  type DropAt = { id: string; pos: "before" | "after" };
  let dragEntry = $state<{ id: string; group: string } | null>(null);
  let dropEntry = $state<DropAt | null>(null);
  let dragProject = $state<string | null>(null);
  let dropProject = $state<DropAt | null>(null);

  /** Which half of the hovered row the cursor is in. */
  function dropSide(ev: DragEvent): "before" | "after" {
    const rect = (ev.currentTarget as HTMLElement).getBoundingClientRect();
    return ev.clientY < rect.top + rect.height / 2 ? "before" : "after";
  }
  /** Splice `dragged` into `ids` relative to `target`/`pos` (target removed first). */
  function placed(ids: string[], dragged: string, target: string, pos: "before" | "after") {
    const rest = ids.filter((id) => id !== dragged);
    let at = rest.indexOf(target);
    if (at < 0) return [...rest, dragged];
    if (pos === "after") at += 1;
    rest.splice(at, 0, dragged);
    return rest;
  }

  function entryDragStart(ev: DragEvent, id: string, group: string) {
    dragEntry = { id, group };
    if (ev.dataTransfer) {
      ev.dataTransfer.effectAllowed = "move";
      // Some webviews only deliver dragover/drop when drag data is set.
      ev.dataTransfer.setData("text/plain", id);
    }
  }
  function entryDragOver(ev: DragEvent, targetId: string, group: string) {
    if (!dragEntry || dragEntry.group !== group) return; // wrong group — not a target
    ev.preventDefault();
    if (dragEntry.id === targetId) {
      dropEntry = null;
      return;
    }
    const pos = dropSide(ev);
    if (dropEntry?.id !== targetId || dropEntry.pos !== pos) dropEntry = { id: targetId, pos };
  }
  function entryDrop(ev: DragEvent, targetId: string, group: string, ordered: PlanEntryResult[]) {
    ev.preventDefault();
    const d = dragEntry;
    const pos = dropEntry?.pos ?? "before";
    dragEntry = null;
    dropEntry = null;
    if (!d || d.group !== group || d.id === targetId) return;
    err(await_(reorderPlan(placed(ordered.map((r) => r.entry.id), d.id, targetId, pos))));
  }

  function projectDragStart(ev: DragEvent, id: string) {
    dragProject = id;
    if (ev.dataTransfer) {
      ev.dataTransfer.effectAllowed = "move";
      ev.dataTransfer.setData("text/plain", id);
    }
  }
  function projectDragOver(ev: DragEvent, targetId: string) {
    if (!dragProject) return;
    ev.preventDefault();
    if (dragProject === targetId) {
      dropProject = null;
      return;
    }
    const pos = dropSide(ev);
    if (dropProject?.id !== targetId || dropProject.pos !== pos) dropProject = { id: targetId, pos };
  }
  function projectDrop(ev: DragEvent, targetId: string) {
    ev.preventDefault();
    const d = dragProject;
    const pos = dropProject?.pos ?? "before";
    dragProject = null;
    dropProject = null;
    if (!d || d === targetId) return;
    const ids = [...data.craftProjects]
      .sort((a, b) => a.sort_key.localeCompare(b.sort_key))
      .map((p) => p.id);
    err(await_(reorderProjects(placed(ids, d, targetId, pos))));
  }

  // Clear all drag state when a drag ends without a drop (so nothing stays
  // dimmed or shows a stale insertion line).
  function dragEnd() {
    dragEntry = null;
    dropEntry = null;
    dragProject = null;
    dropProject = null;
  }
  // Tiny helper so the drop handlers can stay sync but still surface errors.
  function await_(p: Promise<string | null>): string | null {
    p.then(err);
    return null;
  }

  // URL-driven recipe detail (`?bp=`), shared with catalog / wishlist.
  const selectedBp = $derived(page.url.searchParams.get("bp"));
  const selectedCraftable = $derived(
    selectedBp
      ? (craftables.find(
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
{:else if errorMessage && data.craftPlan.length === 0 && data.craftProjects.length === 0}
  <div class="error"><strong>Couldn't load the planner.</strong><p>{errorMessage}</p></div>
{:else if selectedCraftable}
  <div class="detail-wrap">
    <RecipeDetail
      craftable={selectedCraftable}
      backHref={view === "craftable" ? "/crafting?view=craftable" : "/crafting"}
    />
  </div>
{:else}
  <section class="planner" use:keepScroll>
    {#if errorMessage}<p class="inline-error">{errorMessage}</p>{/if}

    <div class="viewtoggle">
      <a class="vt" class:on={view === "plan"} href="?view=plan">Plan</a>
      <a class="vt" class:on={view === "craftable"} href="?view=craftable">Craftable now</a>
    </div>

    {#if view === "plan"}
    <!-- ── Materials rollup ─────────────────────────────────────────── -->
    {#if data.craftPlan.length > 0}
      <div class="rollup">
        <div class="rollup-head">
          <h3>Materials needed</h3>
          <span class="across">
            {#if inactiveCount > 0}active plan · {inactiveCount} project{inactiveCount === 1 ? "" : "s"} parked{:else}across {data.craftPlan.length} planned craft{data.craftPlan.length === 1 ? "" : "s"}{/if}
          </span>
        </div>
        {#if stillNeed.length > 0}
          <div class="stillneed">
            <div class="sn-line">
              <span class="sn-lab">Still need</span>
              {#each stillNeed as m (m.crc)}
                <span class="sn-item">
                  {m.name ?? "Unknown"}
                  <span class="sn-amt">{fmtAmt(m.short, m.kind)}</span>
                  {#if m.neededQuality}<span class="sn-q">Q≥{m.neededQuality}</span>{/if}
                </span>
              {/each}
            </div>
          </div>
        {/if}

        {#if ledger.materials.length === 0}
          <p class="hint">No tracked materials needed by the active plan.</p>
        {:else}
          <div class="mtable" class:lean={!ledger.hasInventory}>
            <div class="mrow mhead">
              <span class="m-name">Material</span>
              <span class="m-num">Need</span>
              {#if ledger.hasInventory}
                <span class="m-num">Have</span>
                <span class="m-num">Reserved</span>
                <span class="m-num">Free</span>
              {/if}
              <span class="m-num">Short</span>
            </div>
            {#each ledger.materials as m (m.crc)}
              <div class="mrow">
                <span class="m-name">{m.name ?? "Unknown"}</span>
                <span class="m-num">{fmtAmt(m.need, m.kind)}</span>
                {#if ledger.hasInventory}
                  <span class="m-num dim">{fmtAmt(m.have, m.kind)}</span>
                  <span class="m-num dim">{fmtAmt(m.reserved, m.kind)}</span>
                  <span class="m-num" class:good={m.free > 0}>{fmtAmt(m.free, m.kind)}</span>
                {/if}
                <span class="m-num">
                  {#if m.short > 0}<span class="buy">{fmtAmt(m.short, m.kind)} ▲</span>
                  {:else}<span class="ok">—</span>{/if}
                </span>
              </div>
            {/each}
          </div>
          {#if !ledger.hasInventory}
            <p class="hint">
              Turn on <a href="/resources">resource sync</a> to see have / reserved / free — for now
              this is the gross shopping list.
            </p>
          {/if}
        {/if}
      </div>
    {/if}

    <!-- ── Add bar ──────────────────────────────────────────────────── -->
    <div class="addbar">
      <button class="btn" onclick={() => openPicker(null)}>+ Add item</button>
      {#if showNewProject}
        <form class="newproj" onsubmit={(e) => { e.preventDefault(); addProject(); }}>
          <!-- svelte-ignore a11y_autofocus -->
          <input class="txt" placeholder="Project name…" bind:value={newProjectName} autofocus
            onblur={() => { if (!newProjectName.trim()) showNewProject = false; }} />
          <button class="btn" type="submit">Add</button>
        </form>
      {:else}
        <button class="btn ghost" onclick={() => (showNewProject = true)}>+ New project</button>
      {/if}
    </div>

    {#if picker.open}
      <div class="picker">
        <div class="picker-head">
          <span class="picker-to">Add to</span>
          <select class="sel" value={picker.target ?? ""} onchange={(e) => (picker.target = e.currentTarget.value || null)}>
            <option value="">Unsorted</option>
            {#each data.craftProjects as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
          </select>
          <!-- svelte-ignore a11y_autofocus -->
          <input class="txt grow" placeholder="Search the catalog…" bind:value={picker.query} autofocus />
          <button class="picker-close" title="Close" onclick={() => (picker.open = false)}>×</button>
        </div>
        {#if pickerResults.length === 0}
          <p class="picker-empty">{picker.query ? "No matches." : "Type to search, or ♡ items in the catalog to see suggestions here."}</p>
        {:else}
          <ul class="picker-list">
            {#each pickerResults as c (c.entityKey)}
              <li>
                <button class="picker-item" onclick={() => pick(c.rep.blueprint_record_guid)}>
                  <span class="pi-add">+</span>
                  <span class="pi-name">{nameOf(c.rep)}</span>
                  <span class="pi-cat">{categoryLabel(c.rep.blueprint_record_guid)}</span>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    {/if}

    <!-- ── The plan ─────────────────────────────────────────────────── -->
    {#if planEmpty}
      <p class="empty">
        Your plan is empty. <strong>+ Add item</strong> to start, or make a project to group a build.
      </p>
    {/if}

    {#each groups as g (g.project?.id ?? "unsorted")}
      {@const gk = groupKey(g.project?.id ?? null)}
      <div class="group" class:parked={g.project && !g.project.active}>
        <div
          class="group-head"
          class:drop-before={g.project && dropProject?.id === g.project.id && dropProject.pos === "before"}
          class:drop-after={g.project && dropProject?.id === g.project.id && dropProject.pos === "after"}
          role="group"
          ondragover={(e) => { if (g.project) projectDragOver(e, g.project.id); }}
          ondrop={(e) => { if (g.project) projectDrop(e, g.project.id); }}
        >
          {#if g.project}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <span class="grip" draggable="true" title="Drag to reorder project"
              ondragstart={(e) => projectDragStart(e, g.project!.id)} ondragend={dragEnd}>⠿</span>
            {#if editingProject === g.project.id}
              <input class="txt rename" bind:value={editName}
                onblur={() => commitRename(g.project!.id)}
                onkeydown={(e) => { if (e.key === "Enter") commitRename(g.project!.id); if (e.key === "Escape") editingProject = null; }} />
            {:else}
              <button class="group-name" onclick={() => startRename(g.project!.id, g.project!.name)} title="Rename">{g.project.name}</button>
            {/if}
          {:else}
            <span class="group-name unsorted">Unsorted</span>
          {/if}
          <span class="group-count">{g.entries.length}</span>
          {#if ledger.hasInventory && g.entries.length > 0 && (!g.project || g.project.active)}
            <span class="group-roll" class:allready={readyCount(g.entries) === g.entries.length}>
              {readyCount(g.entries)}/{g.entries.length} ready
            </span>
          {/if}
          {#if g.project}
            <label class="incl" title="Include this project in the materials rollup and reservation">
              <input type="checkbox" checked={g.project.active}
                onchange={(e) => err(await_(setProjectActive(g.project!.id, e.currentTarget.checked)))} />
              in totals
            </label>
            <button class="group-del" title="Delete project" onclick={() => removeProject(g.project!.id, g.project!.name)}>×</button>
          {/if}
        </div>

        {#if g.entries.length === 0}
          <p class="group-empty">Empty — use <strong>+ Add item</strong> (set the target to this project), or move crafts in.</p>
        {:else}
          <ul class="entries">
            {#each g.entries as r (r.entry.id)}
              {@const e = r.entry}
              {@const st = rowStatus(r)}
              <li class="entry" class:dragging={dragEntry?.id === e.id}
                class:drop-before={dropEntry?.id === e.id && dropEntry.pos === "before"}
                class:drop-after={dropEntry?.id === e.id && dropEntry.pos === "after"}
                ondragover={(e2) => entryDragOver(e2, e.id, gk)}
                ondrop={(e2) => entryDrop(e2, e.id, gk, g.entries)}>
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <span class="grip" draggable="true" title="Drag to reorder"
                  ondragstart={(ev) => entryDragStart(ev, e.id, gk)} ondragend={dragEnd}>⠿</span>
                <span class="e-main">
                  <a class="e-name" href="?bp={e.blueprint_guid}" draggable="false">{nameOfGuid(e.blueprint_guid)}</a>
                  <span class="e-cat">{categoryLabel(e.blueprint_guid)}</span>
                </span>
                <span class="qty" title="How many to make">
                  <button class="step" onclick={() => patch(e, { quantity: Math.max(1, e.quantity - 1) })} aria-label="fewer">−</button>
                  <span class="qty-n">×{e.quantity}</span>
                  <button class="step" onclick={() => patch(e, { quantity: e.quantity + 1 })} aria-label="more">+</button>
                </span>
                <span class="ql" title="Target quality — only materials at or above this count">
                  <span class="ql-lab">Q≥</span>
                  <input class="ql-in" type="number" min="0" max="1000" step="25"
                    value={e.target_quality ?? BASE_QUALITY}
                    onchange={(ev) => setQuality(e, ev.currentTarget.value)} />
                </span>
                <select class="sel proj" title="Project" value={e.project_id ?? ""}
                  onchange={(ev) => patch(e, { project_id: ev.currentTarget.value || null })}>
                  <option value="">Unsorted</option>
                  {#each data.craftProjects as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
                </select>
                <button
                  class="e-status-cell"
                  class:open={expanded.has(e.id)}
                  aria-expanded={expanded.has(e.id)}
                  title="Show material breakdown"
                  onclick={() => toggleExpand(e.id)}
                >
                  {#if st?.state === "ready"}
                    <span class="e-status ready">✓ ready</span>
                  {:else if st?.state === "parked"}
                    <span class="e-status parked">parked</span>
                  {:else if st?.state === "short"}
                    <span class="e-status" class:partial={st.partial} class:short={!st.partial}>
                      <span class="es-lab">short</span>
                      {#each st.items.slice(0, 2) as it, i (i)}<span class="es-mat">{it.name}{#if it.quality}<span class="es-q">Q</span>{/if}</span>{#if i < Math.min(st.items.length, 2) - 1}<span class="es-sep">,</span>{/if}{/each}{#if st.items.length > 2}<span class="es-more">+{st.items.length - 2}</span>{/if}
                    </span>
                  {:else}
                    <span class="e-status det">details</span>
                  {/if}
                  <span class="e-caret" aria-hidden="true">▾</span>
                </button>
                <button class="e-del" title="Remove from plan" onclick={() => err(await_(removePlanEntry(e.id)))}>×</button>
              </li>
              {#if expanded.has(e.id)}
                <li class="entry-detail">
                  {#if r.readiness === "excluded"}
                    <p class="ed-note">Parked — not counted in the rollup or reservation.</p>
                  {/if}
                  {#if r.ingredients.length === 0}
                    <p class="ed-note">No tracked materials for this recipe.</p>
                  {:else}
                    <ul class="ed-list">
                      {#each r.ingredients as ing, i (i)}
                        {@const tracked = r.readiness !== "untracked" && r.readiness !== "excluded"}
                        {@const covered = tracked && ing.short === 0}
                        <li class="ed-row">
                          <span class="ed-name">{ing.name ?? "Unknown"}</span>
                          <span class="ed-val" class:ok={covered} class:short={tracked && !covered}>
                            {fmtAmt(ing.need, ing.kind)}{#if !covered && ing.targetQuality > 0} · Q≥{ing.targetQuality}{/if}{#if tracked && ing.short > 0 && ing.short < ing.need} <span class="ed-deficit">short {fmtAmt(ing.short, ing.kind)}</span>{/if}
                          </span>
                        </li>
                      {/each}
                    </ul>
                  {/if}
                </li>
              {/if}
            {/each}
          </ul>
        {/if}
      </div>
    {/each}
    {:else}
      <!-- ── Craftable now (owned recipes your materials cover) ───────── -->
      {#if !hasInventory}
        <p class="empty">
          Turn on <a href="/resources">resource sync</a> to see which of your owned recipes your
          materials can craft right now.
        </p>
      {:else}
        <div class="group">
          <div class="group-head">
            <span class="group-name">Ready to craft</span>
            <span class="group-count">{readyNow.length}</span>
          </div>
          {#if readyNow.length === 0}
            <p class="group-empty">No owned recipe is fully covered by your current materials.</p>
          {:else}
            <ul class="craft-list">
              {#each readyNow as c (c.entityKey)}
                <li>
                  <a class="craft-row" href="?bp={c.rep.blueprint_record_guid}&view=craftable">
                    <span class="cr-name">{nameOf(c.rep)}</span>
                    <span class="cr-cat">{categoryLabel(c.rep.blueprint_record_guid)}</span>
                    <span class="cr-status ready">✓ have materials</span>
                  </a>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
        {#if almostNow.length > 0}
          <div class="group">
            <div class="group-head">
              <span class="group-name unsorted">Almost</span>
              <span class="group-count">{almostNow.length}</span>
            </div>
            <ul class="craft-list">
              {#each almostNow as c (c.entityKey)}
                {@const x = cov(c)}
                <li>
                  <a class="craft-row" href="?bp={c.rep.blueprint_record_guid}&view=craftable">
                    <span class="cr-name">{nameOf(c.rep)}</span>
                    <span class="cr-cat">{categoryLabel(c.rep.blueprint_record_guid)}</span>
                    {#if x}<span class="cr-status">materials {x.ingredients.filter((i) => i.satisfied).length}/{x.ingredients.length}</span>{/if}
                  </a>
                </li>
              {/each}
            </ul>
          </div>
        {/if}
      {/if}
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
  .inline-error {
    margin: 0 0 0.8rem;
    color: var(--bad);
    font-size: 0.82rem;
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

  /* ── Plan ⇄ Craftable toggle ── */
  .viewtoggle {
    display: inline-flex;
    gap: 0.2rem;
    margin-bottom: 1rem;
    padding: 0.2rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--panel-2);
  }
  .vt {
    padding: 0.25rem 0.9rem;
    border-radius: 999px;
    font-size: 0.8rem;
    color: var(--muted);
    text-decoration: none;
  }
  .vt:hover {
    color: var(--text);
  }
  .vt.on {
    background: var(--ember);
    color: var(--on-ember);
  }

  /* ── Craftable-now rows ── */
  .craft-list {
    list-style: none;
    margin: 0.3rem 0 0;
    padding: 0;
  }
  .craft-row {
    display: flex;
    align-items: center;
    gap: 0.8rem;
    padding: 0.45rem 0.5rem;
    border-radius: 8px;
    border: 1px solid transparent;
    text-decoration: none;
    color: inherit;
  }
  .craft-row:hover {
    background: var(--panel);
    border-color: var(--line);
  }
  .cr-name {
    flex: 0 1 auto;
    font-size: 0.9rem;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .craft-row:hover .cr-name {
    color: var(--ember);
  }
  .cr-cat {
    flex: 0 0 auto;
    font-size: 0.68rem;
    color: var(--faint);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .cr-status {
    margin-left: auto;
    flex: 0 0 auto;
    font-size: 0.72rem;
    color: var(--muted);
    padding: 0.1rem 0.5rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--panel-2);
  }
  .cr-status.ready {
    color: var(--good);
    border-color: var(--good);
  }

  /* ── Materials rollup ── */
  .rollup {
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--panel);
    padding: 0.8rem 1rem;
    margin-bottom: 1.2rem;
  }
  .rollup-head {
    display: flex;
    align-items: baseline;
    gap: 0.6rem;
    margin-bottom: 0.5rem;
  }
  .rollup-head h3 {
    margin: 0;
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--ember);
  }
  .across {
    font-size: 0.72rem;
    color: var(--faint);
  }
  /* "Still need" digest — the actionable shortfalls, above the full table. */
  .stillneed {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    margin-bottom: 0.8rem;
    padding-bottom: 0.7rem;
    border-bottom: 1px dashed var(--line);
  }
  .sn-line {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 0.45rem;
  }
  .sn-lab {
    flex: 0 0 auto;
    font-size: 0.64rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--ember);
    align-self: center;
  }
  .sn-item {
    display: inline-flex;
    align-items: baseline;
    gap: 0.3rem;
    font-size: 0.8rem;
    color: var(--text);
    padding: 0.1rem 0.55rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--panel-2);
  }
  .sn-amt {
    color: var(--ember);
    font-variant-numeric: tabular-nums;
  }
  .sn-q {
    font-size: 0.68rem;
    color: var(--warn);
  }
  .mtable {
    display: grid;
    grid-template-columns: 1fr 7rem 7rem 7rem 7rem 7rem;
    row-gap: 0.25rem;
    font-size: 0.82rem;
  }
  .mtable.lean {
    grid-template-columns: 1fr 7rem 7rem;
  }
  .mrow {
    display: contents;
  }
  .mhead span {
    font-size: 0.66rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--faint);
    padding-bottom: 0.2rem;
  }
  .m-name {
    color: var(--text);
  }
  .m-num {
    text-align: right;
    font-variant-numeric: tabular-nums;
    color: var(--text);
  }
  .m-num.dim {
    color: var(--faint);
  }
  .m-num.good {
    color: var(--good);
  }
  .buy {
    color: var(--ember);
  }
  .ok {
    color: var(--faint);
  }
  .hint {
    margin: 0.6rem 0 0;
    font-size: 0.76rem;
    color: var(--muted);
  }
  .hint a {
    color: var(--ember);
  }

  /* ── Add bar + picker ── */
  .addbar {
    display: flex;
    gap: 0.5rem;
    margin-bottom: 0.8rem;
  }
  .btn {
    padding: 0.3rem 0.7rem;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: var(--panel-2);
    color: var(--muted);
    font-size: 0.8rem;
    cursor: pointer;
  }
  .btn:hover {
    color: var(--ember);
    border-color: var(--ember-dim);
  }
  .btn.ghost {
    background: transparent;
  }
  .txt {
    padding: 0.25rem 0.5rem;
    border-radius: 6px;
    border: 1px solid var(--ember-dim);
    background: var(--panel-2);
    color: var(--text);
    font-size: 0.85rem;
    outline: none;
  }
  .txt.grow {
    flex: 1;
  }
  .newproj {
    display: inline-flex;
    gap: 0.4rem;
  }
  .picker {
    border: 1px solid var(--ember-dim);
    border-radius: 10px;
    background: var(--panel);
    padding: 0.7rem 0.8rem;
    margin-bottom: 1.2rem;
  }
  .picker-head {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .picker-to {
    font-size: 0.72rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--faint);
  }
  .picker-close {
    width: 1.6rem;
    height: 1.6rem;
    border-radius: 6px;
    border: 1px solid var(--line);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    font-size: 1.1rem;
    line-height: 1;
  }
  .picker-close:hover {
    color: var(--bad);
    border-color: var(--bad);
  }
  .picker-empty {
    margin: 0.6rem 0.1rem 0.1rem;
    font-size: 0.8rem;
    color: var(--faint);
  }
  .picker-list {
    list-style: none;
    margin: 0.5rem 0 0;
    padding: 0;
    max-height: 16rem;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
  }
  .picker-item {
    width: 100%;
    display: flex;
    align-items: baseline;
    gap: 0.6rem;
    padding: 0.3rem 0.5rem;
    border-radius: 6px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--text);
    cursor: pointer;
    text-align: left;
    font-size: 0.86rem;
  }
  .picker-item:hover {
    background: var(--panel-2);
    border-color: var(--ember-dim);
  }
  .pi-add {
    color: var(--ember);
    font-weight: 700;
  }
  .pi-name {
    flex: 0 1 auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pi-cat {
    margin-left: auto;
    flex: 0 0 auto;
    font-size: 0.68rem;
    color: var(--faint);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .empty {
    margin: 1rem 0.2rem;
    font-size: 0.85rem;
    color: var(--faint);
    font-style: italic;
  }

  /* ── Project groups ── */
  .group {
    margin-bottom: 1.4rem;
  }
  .group.parked {
    opacity: 0.62;
  }
  .group-head {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.4rem 0.2rem 0.3rem;
    border-bottom: 1px solid var(--line);
  }
  .grip {
    cursor: grab;
    color: var(--faint);
    font-size: 0.9rem;
    user-select: none;
    padding: 0 0.1rem;
  }
  .grip:active {
    cursor: grabbing;
  }
  .group-name {
    background: none;
    border: none;
    padding: 0;
    margin: 0;
    font-size: 0.95rem;
    font-weight: 700;
    letter-spacing: -0.01em;
    color: var(--ember);
    cursor: pointer;
  }
  .group-name.unsorted {
    color: var(--muted);
    cursor: default;
  }
  .group-count {
    font-size: 0.72rem;
    color: var(--faint);
    font-variant-numeric: tabular-nums;
  }
  .group-roll {
    margin-left: 0.4rem;
    font-size: 0.7rem;
    color: var(--muted);
    padding: 0.05rem 0.45rem;
    border: 1px solid var(--line);
    border-radius: 999px;
  }
  .group-roll.allready {
    color: var(--good);
    border-color: var(--good);
  }
  .incl {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    font-size: 0.7rem;
    color: var(--muted);
    cursor: pointer;
  }
  .incl input {
    accent-color: var(--ember);
    cursor: pointer;
  }
  .group-del {
    width: 1.4rem;
    height: 1.4rem;
    display: grid;
    place-items: center;
    border-radius: 6px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--faint);
    cursor: pointer;
    font-size: 1rem;
    line-height: 1;
  }
  .group-del:hover {
    color: var(--bad);
    border-color: var(--bad);
  }
  .group-empty {
    margin: 0.5rem 0.2rem;
    font-size: 0.8rem;
    color: var(--faint);
    font-style: italic;
  }
  .entries {
    list-style: none;
    margin: 0.3rem 0 0;
    padding: 0;
  }
  /* Fixed grid columns so controls never shift between rows. */
  .entry {
    display: grid;
    grid-template-columns: 1.1rem minmax(0, 1fr) 5.2rem 5.2rem 8.5rem 13rem 1.4rem;
    align-items: center;
    gap: 0.6rem;
    padding: 0.35rem 0.4rem;
    border-radius: 8px;
    border: 1px solid transparent;
  }
  .entry:hover {
    background: var(--panel);
    border-color: var(--line);
  }
  .entry.dragging {
    opacity: 0.4;
    outline: 1px dashed var(--ember-dim);
  }
  /* Insertion line showing where a dragged row will land. */
  .entry.drop-before {
    box-shadow: inset 0 2px 0 0 var(--ember);
  }
  .entry.drop-after {
    box-shadow: inset 0 -2px 0 0 var(--ember);
  }
  .group-head.drop-before {
    box-shadow: inset 0 2px 0 0 var(--ember);
  }
  .group-head.drop-after {
    box-shadow: inset 0 -2px 0 0 var(--ember);
  }
  .e-main {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    min-width: 0;
  }
  .e-name {
    font-size: 0.9rem;
    color: var(--text);
    text-decoration: none;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .e-name:hover {
    color: var(--ember);
  }
  .e-cat {
    flex: 0 0 auto;
    font-size: 0.66rem;
    color: var(--faint);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .qty {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    justify-self: end;
  }
  .step {
    width: 1.25rem;
    height: 1.25rem;
    display: grid;
    place-items: center;
    border-radius: 6px;
    border: 1px solid var(--line);
    background: var(--panel-2);
    color: var(--muted);
    cursor: pointer;
    font-size: 0.9rem;
    line-height: 1;
  }
  .step:hover {
    color: var(--ember);
    border-color: var(--ember-dim);
  }
  .qty-n {
    min-width: 2ch;
    text-align: center;
    font-size: 0.8rem;
    font-variant-numeric: tabular-nums;
    color: var(--text);
  }
  .ql {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    justify-self: end;
  }
  .ql-lab {
    font-size: 0.68rem;
    color: var(--faint);
  }
  .ql-in {
    width: 3.4rem;
    padding: 0.18rem 0.3rem;
    border-radius: 6px;
    border: 1px solid var(--line);
    background: var(--panel-2);
    color: var(--text);
    font-size: 0.76rem;
    font-variant-numeric: tabular-nums;
    outline: none;
  }
  .ql-in:focus {
    border-color: var(--ember);
  }
  .sel {
    font-size: 0.74rem;
    color: var(--muted);
    background: var(--panel-2);
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 0.2rem 0.3rem;
    cursor: pointer;
    outline: none;
  }
  .sel:focus {
    border-color: var(--ember);
  }
  .sel.proj {
    width: 100%;
  }
  /* The status cell is a click-to-expand toggle (chip + caret). */
  .e-status-cell {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    min-width: 0;
    overflow: hidden;
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
    text-align: left;
  }
  .e-caret {
    flex: 0 0 auto;
    color: var(--faint);
    font-size: 0.6rem;
    transition: transform 120ms;
  }
  .e-status-cell.open .e-caret {
    transform: rotate(180deg);
  }
  .e-status-cell:hover .e-status {
    border-color: var(--ember-dim);
  }
  .e-status.det {
    color: var(--faint);
  }
  .e-status {
    display: inline-flex;
    align-items: baseline;
    gap: 0.25rem;
    max-width: 100%;
    font-size: 0.7rem;
    color: var(--muted);
    padding: 0.1rem 0.45rem;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--panel-2);
    white-space: nowrap;
    overflow: hidden;
  }
  .es-lab {
    flex: 0 0 auto;
    color: var(--faint);
  }
  .es-mat {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .es-sep {
    flex: 0 0 auto;
    color: var(--faint);
    margin-left: -0.2rem;
  }
  .es-more {
    flex: 0 0 auto;
    color: var(--faint);
  }
  /* Per-material "below required quality" badge — only on quality-short ones. */
  .es-q {
    font-size: 0.5rem;
    font-weight: 700;
    color: var(--warn);
    border: 1px solid var(--warn);
    border-radius: 3px;
    padding: 0 0.12rem;
    margin-left: 0.12rem;
    vertical-align: middle;
  }
  .e-status.ready {
    color: var(--good);
    border-color: var(--good);
  }
  .e-status.partial {
    color: var(--warn);
  }
  .e-status.short {
    color: var(--ember);
  }
  .e-status.parked {
    color: var(--faint);
    font-style: italic;
  }
  .e-del {
    width: 1.4rem;
    height: 1.4rem;
    display: grid;
    place-items: center;
    border-radius: 6px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--faint);
    cursor: pointer;
    font-size: 1rem;
    line-height: 1;
  }
  .e-del:hover {
    color: var(--bad);
    border-color: var(--bad);
  }

  /* Inline material breakdown, revealed under an entry. */
  .entry-detail {
    margin: 0.1rem 0 0.5rem 2.2rem;
    padding: 0.5rem 0.8rem;
    border-left: 2px solid var(--line);
    background: var(--panel);
    border-radius: 0 8px 8px 0;
  }
  .ed-note {
    margin: 0;
    font-size: 0.76rem;
    color: var(--faint);
    font-style: italic;
  }
  .ed-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    column-gap: 1.2rem;
    row-gap: 0.25rem;
  }
  .ed-row {
    display: contents;
  }
  .ed-name {
    font-size: 0.82rem;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* Color is the signal: green = covered, orange = short, muted = untracked. */
  .ed-val {
    font-size: 0.78rem;
    text-align: right;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    color: var(--faint);
  }
  .ed-val.ok {
    color: var(--good);
  }
  .ed-val.short {
    color: var(--ember);
  }
  .ed-deficit {
    font-size: 0.7rem;
    color: var(--faint);
    margin-left: 0.3rem;
  }
</style>
