<script lang="ts">
  import { onMount } from "svelte";
  import { commands, errText, type AppSettings } from "$lib/ipc";
  import AccountManager from "$lib/components/AccountManager.svelte";
  import BlueprintImport from "$lib/components/BlueprintImport.svelte";
  import PageHeader from "$lib/components/PageHeader.svelte";
  import Switch from "$lib/components/Switch.svelte";
  import { openOnboarding } from "$lib/state/onboardingStore.svelte";

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
        text: errText(result.error),
      };
    }
  }
</script>

<PageHeader title="Settings" flush>
  {#snippet subtitle()}Account, sync &amp; preferences{/snippet}
</PageHeader>

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
          <Switch
            checked={settings.sensor_enabled}
            label="Live game-log sensing"
            onchange={(v) => setSensor(v)}
          />
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
      {#if !settings.online_enabled}
        <p class="offline-note">
          Offline mode is on (<strong>Account</strong> tab → Online features) —
          live sync is paused.
        </p>
      {/if}
      <div class="row">
        <Switch
          checked={settings.live_sync_enabled}
          disabled={busy || !settings.online_enabled}
          label="Live blueprint sync"
          onchange={(v) => setEnabled(v)}
        />
        <span class="switch-label">{settings.live_sync_enabled ? "Enabled" : "Disabled"}</span>
      </div>
      {#if settings.live_sync_enabled}
        <div class="row">
          <button class="btn" onclick={syncNow} disabled={syncing || !settings.online_enabled}>
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
      <button class="btn" onclick={openOnboarding}>Re-run onboarding</button>
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
      <button class="btn btn-danger" onclick={wipeCache} disabled={wiping}>
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
      <button class="btn" onclick={() => (showConsent = false)}>Cancel</button>
      <button class="btn btn-primary" onclick={acceptConsent}>I understand — enable</button>
    </div>
  </div>
{/if}

<style>
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
  .page {
    flex: 1;
    overflow-y: auto;
    padding: 1.2rem 1.6rem 2rem;
    display: flex;
    flex-direction: column;
    gap: 1.2rem;
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
  .result {
    font-size: 0.78rem;
  }
  .result.ok {
    color: var(--good);
  }
  .result.err {
    color: var(--bad);
  }
  .offline-note {
    margin: 0.7rem 0 0;
    font-size: 0.8rem;
    color: var(--ember);
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
