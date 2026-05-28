<script lang="ts">
  import { onMount } from "svelte";
  import { commands, type ActiveScope, type BpView } from "$lib/bindings";

  let blueprints = $state<BpView[]>([]);
  let loading = $state(true);
  let errorMessage = $state<string | null>(null);
  let query = $state("");
  let scope = $state<ActiveScope | null>(null);
  let scopeError = $state<string | null>(null);
  let verifying = $state(false);
  let verifyMessage = $state<string | null>(null);

  onMount(async () => {
    // Kick off both in parallel — activeScope triggers the same SC load
    // listBlueprints needs, so they share the cost.
    const [bpResult, scopeResult] = await Promise.all([
      commands.listBlueprints(),
      commands.activeScope(),
    ]);
    if (bpResult.status === "ok") {
      blueprints = bpResult.data;
    } else {
      errorMessage = `${bpResult.error.kind}: ${bpResult.error.message}`;
    }
    if (scopeResult.status === "ok") {
      scope = scopeResult.data;
    } else {
      // Don't fail silently — a broken scope (e.g. stale DB schema, no
      // launcher identity) used to just hide the chip with no hint why.
      scopeError = `${scopeResult.error.kind}: ${scopeResult.error.message}`;
    }
    loading = false;
  });

  async function verify() {
    if (!scope) return;
    verifying = true;
    verifyMessage = null;
    const result = await commands.verifyAccount(scope.account.id);
    if (result.status === "ok") {
      scope = { ...scope, account: result.data };
      verifyMessage = `verified · #${result.data.citizen_record}`;
    } else {
      verifyMessage = `${result.error.kind}: ${result.error.message}`;
    }
    verifying = false;
  }

  function platformLabel(p: ActiveScope["platform"]): string {
    return p === "prod" ? "PU" : "PTU";
  }
  function channelLabel(c: string): string {
    // sc_installs::Channel::display_name() already returns "LIVE",
    // "HOTFIX", "PTU", etc. — show uppercase verbatim.
    return c.toUpperCase();
  }

  let filtered = $derived.by(() => {
    const q = query.toLowerCase().trim();
    if (!q) return blueprints;
    return blueprints.filter((bp) => {
      const name = bp.display_name?.toLowerCase() ?? "";
      const guid = bp.blueprint_record_guid.toLowerCase();
      const pool = bp.pool_name.toLowerCase();
      return name.includes(q) || guid.includes(q) || pool.includes(q);
    });
  });

  let grouped = $derived.by(() => {
    const map = new Map<string, BpView[]>();
    for (const bp of filtered) {
      const list = map.get(bp.pool_name) ?? [];
      list.push(bp);
      map.set(bp.pool_name, list);
    }
    for (const list of map.values()) {
      list.sort((a, b) =>
        (a.display_name ?? a.blueprint_record_guid).localeCompare(
          b.display_name ?? b.blueprint_record_guid,
        ),
      );
    }
    return [...map.entries()].sort(([a], [b]) => a.localeCompare(b));
  });

  let totalCount = $derived(blueprints.length);
  let filteredCount = $derived(filtered.length);
</script>

<header>
  <div class="title-row">
    <h1>Hearth</h1>
    <span class="badge">v0.0.1 · Stage 2.5</span>
    {#if scope}
      <span class="scope-chip" title={`Citizen record: ${scope.account.citizen_record ?? "unverified"} · Enlisted: ${scope.account.enlisted ?? "unverified"}`}>
        <span class="scope-platform">{platformLabel(scope.platform)}</span>
        <span class="scope-sep">·</span>
        <span class="scope-channel">{channelLabel(scope.channel)}</span>
        <span class="scope-sep">·</span>
        <span class="scope-handle">@{scope.account.handle}</span>
        {#if scope.account.last_verified}
          <span class="scope-verified" title={`Last verified ${scope.account.last_verified}`}>✓</span>
        {/if}
      </span>
      <button class="verify-btn" onclick={verify} disabled={verifying}>
        {verifying ? "Verifying…" : (scope.account.last_verified ? "Re-verify" : "Verify")}
      </button>
      {#if verifyMessage}
        <span class="verify-msg">{verifyMessage}</span>
      {/if}
    {:else if scopeError}
      <span class="scope-error" title={scopeError}>⚠ scope unavailable</span>
    {/if}
  </div>
  <p class="muted">Blueprint catalog.</p>
</header>

<section class="toolbar">
  <input
    type="search"
    placeholder="Search name, pool, or GUID…"
    bind:value={query}
    disabled={loading}
  />
  <span class="count">
    {#if loading}
      Loading…
    {:else if errorMessage}
      —
    {:else if query.trim()}
      {filteredCount} / {totalCount}
    {:else}
      {totalCount} blueprints
    {/if}
  </span>
</section>

{#if loading}
  <p class="status">Loading SC reference data… (first run takes ~10 s while
  the Datacore is parsed; subsequent loads are instant)</p>
{:else if errorMessage}
  <div class="error">
    <strong>Couldn't load blueprints.</strong>
    <p>{errorMessage}</p>
    <p class="hint">
      The most common cause: Star Citizen isn't installed (or the RSI
      launcher has never run on this machine). Install/launch SC at least
      once and restart Hearth.
    </p>
  </div>
{:else if grouped.length === 0}
  <p class="status">No blueprints match.</p>
{:else}
  <section class="catalog">
    {#each grouped as [pool, items] (pool)}
      <details open>
        <summary>
          <span class="pool-name">{pool || "(unnamed pool)"}</span>
          <span class="pool-count">{items.length}</span>
        </summary>
        <ul>
          {#each items as bp, i (`${bp.blueprint_record_guid}|${bp.crafted_entity_guid ?? ""}|${i}`)}
            <li>
              <span class="bp-name">
                {bp.display_name ?? bp.blueprint_record_guid}
              </span>
              {#if bp.display_name}
                <span class="bp-guid">{bp.blueprint_record_guid}</span>
              {/if}
              {#if bp.weight !== 1}
                <span class="bp-weight">weight {bp.weight}</span>
              {/if}
            </li>
          {/each}
        </ul>
      </details>
    {/each}
  </section>
{/if}

<style>
  header {
    padding: 1.5rem 2rem 0.5rem;
  }
  .title-row {
    display: flex;
    align-items: baseline;
    gap: 0.75rem;
  }
  h1 {
    font-size: 2rem;
    margin: 0;
    letter-spacing: -0.02em;
  }
  .badge {
    color: #888;
    font-size: 0.75rem;
    padding: 0.15rem 0.5rem;
    border: 1px solid #333;
    border-radius: 4px;
  }
  .scope-chip {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    font-size: 0.75rem;
    padding: 0.15rem 0.55rem;
    background: #1d1d22;
    border: 1px solid #2c2c34;
    border-radius: 4px;
    color: #b8b8c2;
    font-variant-numeric: tabular-nums;
  }
  .scope-platform {
    color: #8bc;
    font-weight: 600;
  }
  .scope-channel {
    color: #ada;
  }
  .scope-handle {
    color: #d8d8e8;
  }
  .scope-sep {
    color: #555;
  }
  .scope-verified {
    color: #6c9;
    margin-left: 0.15rem;
  }
  .verify-btn {
    font-size: 0.7rem;
    padding: 0.15rem 0.5rem;
    background: transparent;
    color: #aab;
    border: 1px solid #333;
    border-radius: 4px;
    cursor: pointer;
  }
  .verify-btn:hover:not(:disabled) {
    background: #1d1d22;
    color: #ccd;
    border-color: #4d6cf3;
  }
  .verify-btn:disabled {
    opacity: 0.5;
    cursor: progress;
  }
  .verify-msg {
    color: #888;
    font-size: 0.7rem;
  }
  .scope-error {
    color: #f5a;
    font-size: 0.72rem;
    cursor: help;
  }
  .muted {
    color: #888;
    font-size: 0.85rem;
    margin: 0.25rem 0 0;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 1rem 2rem;
    position: sticky;
    top: 0;
    background: #131316;
    border-bottom: 1px solid #222;
    z-index: 1;
  }
  input[type="search"] {
    flex: 1;
    padding: 0.5rem 0.75rem;
    background: #1d1d22;
    border: 1px solid #333;
    border-radius: 6px;
    outline: none;
    transition: border-color 80ms;
  }
  input[type="search"]:focus {
    border-color: #4d6cf3;
  }
  input[type="search"]:disabled {
    opacity: 0.6;
  }
  .count {
    color: #888;
    font-variant-numeric: tabular-nums;
    font-size: 0.85rem;
    white-space: nowrap;
  }

  .status,
  .error {
    padding: 1rem 2rem;
    color: #999;
  }
  .error strong {
    color: #f5a;
  }
  .error .hint {
    color: #777;
    font-size: 0.85rem;
    margin: 0.5rem 0 0;
  }

  .catalog {
    padding: 0 2rem 3rem;
  }
  details {
    margin: 0.75rem 0;
    border: 1px solid #222;
    border-radius: 6px;
    overflow: hidden;
  }
  summary {
    padding: 0.6rem 0.85rem;
    cursor: pointer;
    background: #1a1a1f;
    display: flex;
    align-items: center;
    gap: 0.75rem;
    user-select: none;
  }
  summary:hover {
    background: #20202a;
  }
  .pool-name {
    flex: 1;
    font-weight: 500;
  }
  .pool-count {
    color: #888;
    font-size: 0.8rem;
    font-variant-numeric: tabular-nums;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0.25rem 0;
  }
  li {
    padding: 0.4rem 1.25rem;
    display: flex;
    align-items: baseline;
    gap: 0.75rem;
    font-size: 0.9rem;
  }
  li:nth-child(even) {
    background: #16161a;
  }
  .bp-name {
    flex: 1;
  }
  .bp-guid {
    color: #555;
    font-family: ui-monospace, "SF Mono", Consolas, monospace;
    font-size: 0.75rem;
  }
  .bp-weight {
    color: #888;
    font-size: 0.75rem;
    font-variant-numeric: tabular-nums;
  }
</style>
