<script lang="ts">
  import { onMount } from "svelte";
  import { type DiscoveredIdentity } from "$lib/bindings";
  import {
    bpImport,
    scan,
    applyImport,
    setChoice,
    loadAccounts,
  } from "$lib/importStore.svelte";

  // View over the persistent import store ($lib/blueprintImport.svelte). The
  // scan/import state lives there, not here, so it survives navigation: switch
  // away mid-scan and the progress (and result) are still shown on return.

  onMount(loadAccounts);

  const renameChain = (id: DiscoveredIdentity) =>
    id.handles.length > 1 ? id.handles.join(" → ") : (id.handles[0] ?? "unknown");
</script>

<div class="card">
  <h2>Import from game logs</h2>
  <p class="muted">
    Reads your local Star Citizen logs (<code>Game.log</code> + the
    <code>logbackups/</code> folder) for blueprints you
    <strong>received</strong>
    in recorded sessions and marks them owned. ToS-safe and works offline; you confirm
    which RSI account each batch belongs to before anything is marked.
    <br /><strong>Limitation:</strong> it only sees blueprints <em>received</em>
    during sessions that were logged — it misses default-unlocked blueprints and
    any session with no saved log. So it's a bulk head-start, not your full list;
    top it up with live sync or by ticking ✓ yourself. Persistent-universe sessions
    only (PTU / test shards are skipped).
  </p>
  {#if bpImport.error}<p class="err">{bpImport.error}</p>{/if}
  <div class="row">
    <button onclick={scan} disabled={bpImport.scanning || bpImport.importing}>
      {#if bpImport.scanning}<span class="spinner" aria-hidden="true"></span>{/if}
      {bpImport.scanning ? "Scanning logs…" : "Scan game logs"}
    </button>
    {#if bpImport.importResult}
      <span class="result ok">
        Imported: {bpImport.importResult.newly_owned} newly owned across {bpImport.importResult.accounts_touched}
        account{bpImport.importResult.accounts_touched === 1 ? "" : "s"}{bpImport.importResult.unresolved.length
          ? ` · ${bpImport.importResult.unresolved.length} unrecognised`
          : ""}
      </span>
    {/if}
  </div>
  {#if bpImport.scanning}
    <p class="scan-note">
      Reading <code>Game.log</code> + every file in <code>logbackups/</code> — this
      can take a moment. You can leave this page; the scan keeps running.
    </p>
  {/if}

  {#if bpImport.identities}
    {#if bpImport.identities.length === 0}
      <p class="muted">No blueprint history found in the logs.</p>
    {:else}
      <ul class="ident-list">
        {#each bpImport.identities as id (id.key)}
          <li class="ident">
            <div class="ident-info">
              <span class="ident-handles">{renameChain(id)}</span>
              <span class="ident-meta">
                {id.blueprint_count} blueprint{id.blueprint_count === 1 ? "" : "s"} · {id.session_count}
                session{id.session_count === 1 ? "" : "s"}
                {#if id.account_hint}· id {id.account_hint}{/if}
              </span>
            </div>
            <select
              value={bpImport.choice[id.key]}
              onchange={(e) => setChoice(id.key, e.currentTarget.value)}
            >
              <option value="__ignore__">Ignore</option>
              <option value="__new__">New account</option>
              {#each bpImport.accounts as a (a.id)}
                <option value={a.id}>→ @{a.handle}</option>
              {/each}
            </select>
          </li>
        {/each}
      </ul>
      <div class="row">
        <button class="primary" onclick={applyImport} disabled={bpImport.importing}>
          {#if bpImport.importing}<span class="spinner" aria-hidden="true"></span>{/if}
          {bpImport.importing ? "Importing…" : "Apply import"}
        </button>
        <span class="muted hint"
          >Owned blueprints are marked in the prod scope; this can't un-own anything.</span
        >
      </div>
    {/if}
  {/if}
</div>

<style>
  .card {
    border: 1px solid var(--line);
    border-radius: 10px;
    padding: 1rem 1.1rem;
    background: var(--panel);
  }
  .card h2 {
    margin: 0 0 0.5rem;
    font-size: 1rem;
  }
  .muted {
    color: var(--muted);
    font-size: 0.83rem;
    margin: 0.3rem 0;
  }
  code {
    background: var(--panel-2);
    padding: 0.05rem 0.3rem;
    border-radius: 4px;
    font-size: 0.9em;
  }
  .err {
    color: var(--bad);
    font-size: 0.82rem;
    margin: 0.3rem 0;
  }
  select {
    background: var(--panel-2);
    color: var(--text);
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 0.3rem 0.5rem;
    font-size: 0.82rem;
  }
  button {
    display: inline-flex;
    align-items: center;
    padding: 0.32rem 0.7rem;
    border-radius: 6px;
    border: 1px solid var(--line);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    font-size: 0.82rem;
  }
  button:hover:not(:disabled) {
    color: var(--text);
    border-color: var(--ember-dim);
  }
  button:disabled {
    opacity: 0.7;
    cursor: progress;
  }
  button.primary {
    background: var(--ember);
    border-color: var(--ember);
    color: #1a1209;
    font-weight: 600;
  }
  /* In-button progress ring — inherits the button's text colour. */
  .spinner {
    display: inline-block;
    width: 0.8rem;
    height: 0.8rem;
    margin-right: 0.4rem;
    border-radius: 50%;
    border: 2px solid currentColor;
    border-top-color: transparent;
    animation: spin 0.7s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .scan-note {
    font-size: 0.78rem;
    color: var(--ember);
    margin: 0.5rem 0 0;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 0.7rem;
    flex-wrap: wrap;
    margin-top: 0.5rem;
  }
  .result.ok {
    font-size: 0.8rem;
    color: var(--good);
  }
  .hint {
    font-size: 0.75rem;
  }
  .ident-list {
    list-style: none;
    margin: 0.7rem 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }
  .ident {
    display: flex;
    align-items: center;
    gap: 0.8rem;
    padding: 0.5rem 0.6rem;
    border: 1px solid var(--line);
    border-radius: 8px;
  }
  .ident-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
  }
  .ident-handles {
    font-size: 0.88rem;
    font-weight: 500;
  }
  .ident-meta {
    font-size: 0.73rem;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .ident select {
    flex: 0 0 auto;
  }
</style>
