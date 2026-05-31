<script lang="ts">
  import { commands } from "$lib/bindings";

  let wiping = $state(false);
  let lastResult = $state<{ kind: "ok" | "err"; text: string } | null>(null);

  async function wipeCache() {
    if (wiping) return;
    wiping = true;
    lastResult = null;
    const result = await commands.wipeScCache();
    // On success the backend calls AppHandle::restart() and the
    // process is replaced — the IPC reply usually never arrives.
    // Set a message anyway in case the restart is slow or it's a
    // dev-server reload where the message is briefly visible.
    if (result.status === "ok") {
      lastResult = { kind: "ok", text: "Cache wiped — restarting…" };
    } else {
      wiping = false;
      lastResult = {
        kind: "err",
        text: `${result.error.kind}: ${result.error.message}`,
      };
    }
  }
</script>

<header class="topbar">
  <div class="page-title">
    <h1>Settings</h1>
    <span class="subtitle">App preferences + debug</span>
  </div>
</header>

<section class="page">
  <div class="card">
    <h2>Coming soon</h2>
    <p class="muted">
      Account management (multi-RSI-account picker, profile re-verify),
      platform / channel selection, and app preferences will live here.
    </p>
  </div>

  <div class="card">
    <h2>Debug · SC reference cache</h2>
    <p class="muted">
      Wipe the snapshot cache at <code>%APPDATA%/hearth/cache/</code>
      (every channel's <code>catalog.cook</code> +
      <code>extract.snap</code>). Hearth will restart and rebuild from
      your live <code>Data.p4k</code> — first launch takes ~30 s,
      subsequent loads are sub-second again. Personal data (owned
      blueprints, accounts) is untouched.
    </p>
    <div class="row">
      <button class="danger" onclick={wipeCache} disabled={wiping}>
        {wiping ? "Wiping…" : "Wipe SC cache & restart"}
      </button>
      {#if lastResult}
        <span class="result {lastResult.kind}">{lastResult.text}</span>
      {/if}
    </div>
  </div>
</section>

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
  }
  .page {
    flex: 1;
    overflow-y: auto;
    padding: 1.2rem 1.6rem 2rem;
    display: flex;
    flex-direction: column;
    gap: 1.2rem;
    max-width: 760px;
  }
  .card {
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 10px;
    padding: 1rem 1.1rem;
  }
  .card h2 {
    margin: 0 0 0.5rem;
    font-size: 0.95rem;
    color: var(--text);
    font-weight: 600;
    letter-spacing: -0.005em;
  }
  .muted {
    margin: 0;
    color: var(--muted);
    font-size: 0.85rem;
    line-height: 1.5;
  }
  .muted code {
    font-family: ui-monospace, "SF Mono", Consolas, monospace;
    font-size: 0.78rem;
    background: var(--panel-2);
    padding: 0.05rem 0.3rem;
    border-radius: 4px;
    color: var(--text);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 0.8rem;
    margin-top: 0.8rem;
  }
  button.danger {
    font-size: 0.82rem;
    padding: 0.4rem 0.9rem;
    background: transparent;
    color: var(--bad);
    border: 1px solid var(--line);
    border-radius: 6px;
    cursor: pointer;
    transition: all 90ms;
  }
  button.danger:hover:not(:disabled) {
    border-color: var(--bad);
    background: rgba(255, 90, 130, 0.08);
  }
  button.danger:disabled {
    opacity: 0.5;
    cursor: progress;
  }
  .result {
    font-size: 0.78rem;
  }
  .result.ok {
    color: var(--good);
  }
  .result.err {
    color: var(--bad);
  }
</style>
