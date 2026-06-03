<script lang="ts">
  import { bpImport, runImport } from "$lib/state/importStore.svelte";

  // One-click view over the shared import store ($lib/state/importStore.svelte). Same
  // action onboarding uses — scan + auto-map + apply, in the background, with a
  // notification when done.
</script>

<div class="card">
  <h2>Import from game logs</h2>
  <p class="muted">
    Reads your local Star Citizen logs (<code>Game.log</code> + the
    <code>logbackups/</code> folder) for blueprints you
    <strong>received</strong> in recorded sessions and marks them owned —
    auto-matched to your account(s). ToS-safe, works offline, and runs in the
    background.
    <br /><strong>Limitation:</strong> it only sees blueprints <em>received</em>
    during sessions that were logged — it misses default-unlocked blueprints and
    any session with no saved log. A bulk head-start, not your full list; top it
    up with live sync or by ticking ✓ yourself. Persistent-universe sessions only
    (PTU / test shards skipped).
  </p>
  {#if bpImport.error}<p class="err">{bpImport.error}</p>{/if}
  <div class="row">
    <button onclick={() => runImport()} disabled={bpImport.running}>
      {#if bpImport.running}<span class="spinner" aria-hidden="true"></span>{/if}
      {bpImport.running ? "Importing…" : "Import from game logs"}
    </button>
    {#if bpImport.result}
      <span class="result ok">
        Imported {bpImport.result.newly_owned} newly owned across {bpImport.result.accounts_touched}
        account{bpImport.result.accounts_touched === 1 ? "" : "s"}{bpImport.result.unresolved.length
          ? ` · ${bpImport.result.unresolved.length} unrecognised`
          : ""}
      </span>
    {/if}
  </div>
  {#if bpImport.running}
    <p class="scan-note">
      Reading <code>Game.log</code> + every file in <code>logbackups/</code> — this
      can take a moment. You can leave this page; it keeps running and you'll get a
      notification when it's done.
    </p>
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
</style>
