<script lang="ts">
  import { onMount } from "svelte";
  import { commands, errText, type AppSettings } from "$lib/ipc";
  import AccountManager from "$lib/components/AccountManager.svelte";
  import PageHeader from "$lib/components/PageHeader.svelte";
  import Switch from "$lib/components/Switch.svelte";
  import { openOnboarding } from "$lib/state/onboardingStore.svelte";

  let tab = $state<"account" | "tracking" | "advanced">("account");

  // ── Live game-log tracking (the sensor) ──────────────────────────────
  let scanning = $state(false);
  let lastScan = $state<string | null>(null);

  let wiping = $state(false);
  let lastResult = $state<{ kind: "ok" | "err"; text: string } | null>(null);

  // ── Live blueprint sync ──────────────────────────────────────────────
  let settings = $state<AppSettings | null>(null);
  let showConsent = $state(false);
  let busy = $state(false); // enable/disable in flight
  let syncing = $state(false);
  let lastSync = $state<string | null>(null);

  // ── Live resource-inventory sync (shares the live-sync consent) ───────
  let invBusy = $state(false);
  let invSyncing = $state(false);
  let lastInvSync = $state<string | null>(null);
  let showInvConsent = $state(false);

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

  async function setInventoryEnabled(enabled: boolean) {
    if (invBusy || !settings) return;
    // First enable needs the one-time consent (shared with blueprint sync).
    if (enabled && !settings.live_sync_consented) {
      showInvConsent = true;
      return;
    }
    invBusy = true;
    const r = await commands.setLiveInventory(enabled);
    if (r.status === "ok") settings = r.data;
    invBusy = false;
    if (enabled) inventorySyncNow();
  }

  async function acceptInvConsent() {
    showInvConsent = false;
    invBusy = true;
    const r = await commands.setLiveInventory(true);
    if (r.status === "ok") settings = r.data;
    invBusy = false;
    inventorySyncNow();
  }

  async function inventorySyncNow() {
    if (invSyncing) return;
    invSyncing = true;
    lastInvSync = null;
    const r = await commands.inventorySyncNow();
    if (r.status === "ok") {
      const d = r.data;
      lastInvSync = `${d.resources} resource${d.resources === 1 ? "" : "s"} · ${d.items} item${d.items === 1 ? "" : "s"} · ${d.total_scu.toFixed(1)} SCU`;
    }
    invSyncing = false;
  }

  async function setSensor(enabled: boolean) {
    if (!settings) return;
    const r = await commands.setSensor(enabled);
    if (r.status === "ok") settings = r.data;
    if (enabled) scanLogsNow(); // first scan right away so the toggle does something
  }

  async function scanLogsNow() {
    if (scanning) return;
    scanning = true;
    lastScan = null;
    const r = await commands.scanLogsNow();
    if (r.status === "ok") {
      const d = r.data;
      lastScan = `${d.newly_owned} marked${d.unresolved.length ? ` · ${d.unresolved.length} unrecognised` : ""}`;
    }
    scanning = false;
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
  <button class="tab" class:active={tab === "tracking"} onclick={() => (tab = "tracking")}>Tracking</button>
  <button class="tab" class:active={tab === "advanced"} onclick={() => (tab = "advanced")}>Advanced</button>
</div>

<section class="page">
  {#if tab === "account"}
    <AccountManager />
  {:else if tab === "tracking"}
    {#if settings}
      <div class="card">
        <h2>Game-log tracking</h2>
        <p class="muted">
          Reads your local Star Citizen logs (<code>Game.log</code> + the
          <code>logbackups/</code> folder) for blueprints you
          <strong>received</strong> and marks them owned — for your active account.
          ToS-safe, works offline. It <strong>catches up at startup</strong> and
          keeps marking blueprints live while you play; press <em>Scan now</em> to
          re-scan on demand.
          <br /><strong>Limitation:</strong> it only sees blueprints
          <em>received</em> in logged sessions — it misses default-unlocked
          blueprints and any session with no saved log. Persistent-universe sessions
          only (PTU / test shards skipped).
        </p>
        <div class="row">
          <Switch
            checked={settings.sensor_enabled}
            label="Game-log tracking"
            onchange={(v) => setSensor(v)}
          />
          <span class="switch-label">{settings.sensor_enabled ? "On" : "Off"}</span>
        </div>
        <div class="row">
          <button class="btn" onclick={scanLogsNow} disabled={scanning}>
            {scanning ? "Scanning…" : "Scan now"}
          </button>
          {#if lastScan}<span class="result ok">{lastScan}</span>{/if}
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
    <div class="card">
      <h2>Live resource sync <span class="adv">advanced</span></h2>
      <p class="muted">
        Reads your in-game <strong>resource inventory</strong> from your RSI
        account on CIG's servers — every stowed material (type, quality, amount)
        and where it sits — so the <strong>Resources</strong> page and Wishlist
        can tell you whether you have the mats to craft a want-item.
        <br /><strong>Limitation:</strong> same <strong>unofficial, read-only</strong>
        connection as live blueprint sync, <strong>against Star Citizen's Terms
        of Service</strong> — your own account only, at your own risk. Syncs at
        startup and when you press <em>Sync now</em> — never in the background.
      </p>
      {#if !settings.online_enabled}
        <p class="offline-note">
          Offline mode is on (<strong>Account</strong> tab → Online features) —
          resource sync is paused.
        </p>
      {/if}
      <div class="row">
        <Switch
          checked={settings.live_inventory_enabled}
          disabled={invBusy || !settings.online_enabled}
          label="Live resource sync"
          onchange={(v) => setInventoryEnabled(v)}
        />
        <span class="switch-label">{settings.live_inventory_enabled ? "Enabled" : "Disabled"}</span>
      </div>
      {#if settings.live_inventory_enabled}
        <div class="row">
          <button class="btn" onclick={inventorySyncNow} disabled={invSyncing || !settings.online_enabled}>
            {invSyncing ? "Syncing…" : "Sync now"}
          </button>
          {#if lastInvSync}<span class="result ok">{lastInvSync}</span>{/if}
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

{#if showInvConsent}
  <button class="modal-backdrop" aria-label="Cancel" onclick={() => (showInvConsent = false)}></button>
  <div class="modal" role="dialog" aria-label="Enable live resource sync">
    <h3>Enable live resource sync?</h3>
    <p>
      This connects to CIG's game servers using your RSI launcher session to read
      your in-game resource inventory. It is an <strong>unofficial</strong>
      connection — <strong>against Star Citizen's Terms of Service</strong>. It's
      read-only and only ever touches your own account, but you use it at your own
      risk.
    </p>
    <div class="modal-actions">
      <button class="btn" onclick={() => (showInvConsent = false)}>Cancel</button>
      <button class="btn btn-primary" onclick={acceptInvConsent}>I understand — enable</button>
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
