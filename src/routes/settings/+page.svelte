<script lang="ts">
  import { onMount } from "svelte";
  import { commands, type AppSettings } from "$lib/bindings";
  import AccountManager from "$lib/AccountManager.svelte";
  import BlueprintImport from "$lib/BlueprintImport.svelte";
  import { openOnboarding } from "$lib/onboardingStore.svelte";

  let tab = $state<"account" | "import" | "advanced">("account");

  let wiping = $state(false);
  let lastResult = $state<{ kind: "ok" | "err"; text: string } | null>(null);

  // ── Live blueprint sync ──────────────────────────────────────────────
  let settings = $state<AppSettings | null>(null);
  let showConsent = $state(false);
  let busy = $state(false); // enable/disable in flight
  let syncing = $state(false);
  let lastSync = $state<string | null>(null);

  onMount(async () => {
    const r = await commands.getSettings();
    if (r.status === "ok") settings = r.data;
  });

  async function setEnabled(enabled: boolean) {
    if (busy || !settings) return;
    // First enable needs the one-time consent dialog.
    if (enabled && !settings.live_sync_consented) {
      showConsent = true;
      return;
    }
    busy = true;
    const r = await commands.setLiveSync(enabled);
    if (r.status === "ok") settings = r.data;
    busy = false;
    if (enabled) syncNow(); // first sync right away so the toggle does something
  }

  async function acceptConsent() {
    showConsent = false;
    busy = true;
    const r = await commands.setLiveSync(true);
    if (r.status === "ok") settings = r.data;
    busy = false;
    syncNow();
  }

  async function setSensor(enabled: boolean) {
    if (!settings) return;
    const r = await commands.setSensor(enabled);
    if (r.status === "ok") settings = r.data;
  }

  async function syncNow() {
    if (syncing) return;
    syncing = true;
    lastSync = null;
    const r = await commands.liveSyncNow();
    // Success + failure both fire a notification; show a compact inline echo
    // on success too.
    if (r.status === "ok") {
      const d = r.data;
      lastSync = `${d.total} owned · +${d.added} / −${d.removed}${d.unresolved ? ` · ${d.unresolved} unmatched` : ""}`;
    }
    syncing = false;
  }

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
    <span class="subtitle">Account, sync &amp; preferences</span>
  </div>
</header>

<div class="tabs">
  <button class="tab" class:active={tab === "account"} onclick={() => (tab = "account")}>Account</button>
  <button class="tab" class:active={tab === "import"} onclick={() => (tab = "import")}>Blueprint import</button>
  <button class="tab" class:active={tab === "advanced"} onclick={() => (tab = "advanced")}>Advanced</button>
</div>

<section class="page">
  {#if tab === "account"}
    <AccountManager />
  {:else if tab === "import"}
    <BlueprintImport />
    {#if settings}
      <div class="card">
        <h2>Live game-log sensing</h2>
        <p class="muted">
          While you play, Hearth watches <code>Game.log</code> and marks blueprints
          owned the moment you receive one (with a toast) — the same ToS-safe local
          read as the import above, just continuous. Only catches blueprints
          <em>received</em> during play.
        </p>
        <div class="row">
          <button
            class="switch"
            class:on={settings.sensor_enabled}
            role="switch"
            aria-checked={settings.sensor_enabled}
            aria-label="Live game-log sensing"
            onclick={() => setSensor(!settings!.sensor_enabled)}
          >
            <span class="knob"></span>
          </button>
          <span class="switch-label">{settings.sensor_enabled ? "On" : "Off"}</span>
        </div>
      </div>
    {/if}
    {#if settings?.live_sync_available}
    <div class="card">
      <h2>Live blueprint sync <span class="adv">advanced</span></h2>
      <p class="muted">
        Fetches your <strong>complete</strong> blueprint library straight from your
        RSI account on CIG's servers — the authoritative list, including
        default-unlocked blueprints the log import can't see. Each sync
        <strong>reconciles</strong> your owned set to match your account exactly:
        it adds what's missing <em>and removes what you no longer have</em>.
        <br /><strong>Limitation:</strong> this is an
        <strong>unofficial, read-only</strong> connection to CIG's backend,
        <strong>against Star Citizen's Terms of Service</strong>. It only ever
        touches your own account, but you use it at your own risk. Syncs at
        startup and when you press <em>Sync now</em> — never in the background.
      </p>
      <div class="row">
        <button
          class="switch"
          class:on={settings.live_sync_enabled}
          disabled={busy}
          role="switch"
          aria-checked={settings.live_sync_enabled}
          aria-label="Live blueprint sync"
          onclick={() => setEnabled(!settings!.live_sync_enabled)}
        >
          <span class="knob"></span>
        </button>
        <span class="switch-label">{settings.live_sync_enabled ? "Enabled" : "Disabled"}</span>
      </div>
      {#if settings.live_sync_enabled}
        <div class="row">
          <button class="action-btn" onclick={syncNow} disabled={syncing}>
            {syncing ? "Syncing…" : "Sync now"}
          </button>
          {#if lastSync}<span class="result ok">{lastSync}</span>{/if}
        </div>
      {/if}
    </div>
    {/if}
  {:else if tab === "advanced"}
  <div class="card">
    <h2>First-launch setup</h2>
    <p class="muted">Re-run the welcome walkthrough (account confirmation + tracking setup).</p>
    <div class="row">
      <button class="action-btn" onclick={openOnboarding}>Re-run onboarding</button>
    </div>
  </div>
  <div class="card">
    <h2>Debug · SC reference cache</h2>
    <p class="muted">
      Wipe the snapshot cache at <code>%APPDATA%/hearth/cache/</code>
      (every channel's <code>catalog.cook</code> +
      <code>extract.snap</code>). Hearth will restart and rebuild from
      your live <code>Data.p4k</code>. Personal data (owned
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
  {/if}
</section>

{#if showConsent}
  <button class="modal-backdrop" aria-label="Cancel" onclick={() => (showConsent = false)}></button>
  <div class="modal" role="dialog" aria-label="Enable live blueprint sync">
    <h3>Enable live blueprint sync?</h3>
    <p>
      This connects to CIG's game servers using your RSI launcher session to read
      your owned blueprints. It is an <strong>unofficial</strong> connection —
      <strong>against Star Citizen's Terms of Service</strong>. It's read-only and
      only ever touches your own account, but you use it at your own risk.
    </p>
    <div class="modal-actions">
      <button class="action-btn" onclick={() => (showConsent = false)}>Cancel</button>
      <button class="action-btn primary" onclick={acceptConsent}>I understand — enable</button>
    </div>
  </div>
{/if}

<style>
  .topbar {
    display: flex;
    align-items: center;
    gap: 1.5rem;
    padding: 1.1rem 1.6rem 0.6rem;
  }
  .tabs {
    display: flex;
    gap: 0.2rem;
    padding: 0 1.6rem;
    border-bottom: 1px solid var(--line);
  }
  .tab {
    padding: 0.5rem 0.85rem;
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--muted);
    font-size: 0.85rem;
    cursor: pointer;
    transition: color 90ms, border-color 90ms;
  }
  .tab:hover {
    color: var(--text);
  }
  .tab.active {
    color: var(--ember);
    border-bottom-color: var(--ember);
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

  /* ── Live blueprint sync ── */
  .adv {
    font-size: 0.6rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--faint);
    border: 1px solid var(--line);
    border-radius: 4px;
    padding: 0.05rem 0.35rem;
    margin-left: 0.4rem;
    vertical-align: middle;
  }
  .action-btn {
    font-size: 0.82rem;
    padding: 0.4rem 0.9rem;
    background: transparent;
    color: var(--muted);
    border: 1px solid var(--line);
    border-radius: 6px;
    cursor: pointer;
    transition: all 90ms;
  }
  .action-btn:hover:not(:disabled) {
    color: var(--text);
    border-color: var(--ember-dim);
  }
  .action-btn:disabled {
    opacity: 0.5;
    cursor: progress;
  }
  .action-btn.primary,
  .action-btn.primary:hover:not(:disabled) {
    background: var(--ember);
    border-color: var(--ember);
    color: #1a1209;
    font-weight: 600;
  }

  .switch {
    width: 2.2rem;
    height: 1.2rem;
    flex: 0 0 auto;
    padding: 0;
    border-radius: 999px;
    border: 1px solid var(--line);
    background: var(--panel-2);
    cursor: pointer;
    position: relative;
    transition: background 120ms, border-color 120ms;
  }
  .switch.on {
    background: var(--ember-glow);
    border-color: var(--ember-dim);
  }
  .switch .knob {
    position: absolute;
    top: 1px;
    left: 1px;
    width: 1rem;
    height: 1rem;
    border-radius: 50%;
    background: var(--muted);
    transition: transform 120ms, background 120ms;
  }
  .switch.on .knob {
    transform: translateX(1rem);
    background: var(--ember);
  }
  .switch:disabled {
    opacity: 0.6;
    cursor: progress;
  }
  .switch-label {
    font-size: 0.82rem;
    color: var(--muted);
  }

  /* Consent modal. */
  .modal-backdrop {
    position: fixed;
    inset: 0;
    z-index: 80;
    background: rgba(0, 0, 0, 0.5);
    border: none;
    padding: 0;
    cursor: default;
  }
  .modal {
    position: fixed;
    z-index: 81;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: min(440px, 90vw);
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 12px;
    padding: 1.2rem 1.3rem;
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.5);
  }
  .modal h3 {
    margin: 0 0 0.6rem;
    font-size: 1rem;
  }
  .modal p {
    margin: 0;
    color: var(--muted);
    font-size: 0.85rem;
    line-height: 1.5;
  }
  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.6rem;
    margin-top: 1.1rem;
  }
</style>
