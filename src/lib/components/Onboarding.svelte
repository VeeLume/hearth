<script lang="ts">
  import { onMount } from "svelte";
  import { commands, errText, type ActiveScope, type AppSettings } from "$lib/ipc";
  import { finishOnboarding } from "$lib/state/onboardingStore.svelte";
  import Switch from "$lib/components/Switch.svelte";
  import Avatar from "$lib/components/Avatar.svelte";

  // First-launch flow: Welcome → Your account → Tracking. Lean by design —
  // confirms the account and sets the data source; importing history and live
  // sync live in Settings. Step 0 doubles as cover for the background SC parse.

  let step = $state(0);
  let scope = $state<ActiveScope | null>(null);
  let scopeError = $state<string | null>(null);
  let noInstall = $state(false);
  let verifying = $state(false);
  let settings = $state<AppSettings | null>(null);
  let showConsent = $state(false); // inline ToS consent for live blueprint sync
  let showInvConsent = $state(false); // inline ToS consent for live resource sync
  const onlineEnabled = $derived(settings?.online_enabled ?? true);

  onMount(() => {
    loadSettings();
    loadScope();
  });

  async function loadSettings() {
    const set = await commands.getSettings();
    if (set.status === "ok") settings = set.data;
  }

  // Cold-start resilience: `active_scope` needs the db pool warmed, so the very
  // first call can transiently fail (Storage/Internal) before warmup settles.
  // Retry a few times before surfacing an error, and treat a genuine "no SC
  // install" (NoInstall) as terminal rather than spinning on it. While retrying
  // the UI stays on its "Looking for your account…" state.
  async function loadScope() {
    const RETRIES = 5;
    const DELAY_MS = 300;
    for (let attempt = 0; attempt <= RETRIES; attempt++) {
      const s = await commands.activeScope();
      if (s.status === "ok") {
        scope = s.data;
        scopeError = null;
        noInstall = false;
        return;
      }
      if (s.error.kind === "NoInstall") {
        noInstall = true;
        scopeError = null;
        return;
      }
      if (attempt < RETRIES) {
        await new Promise((r) => setTimeout(r, DELAY_MS));
      } else {
        scopeError = errText(s.error);
      }
    }
  }

  // Best-effort: capture the immutable RSI anchors (citizen record) by scraping
  // the public profile. Only ever called when the user leaves the account step
  // with online features still enabled, so the toggle is seen and changeable
  // BEFORE any network call. Silent on failure — never blocks.
  async function captureAnchor() {
    if (!scope || verifying || scope.account.last_verified) return;
    verifying = true;
    const r = await commands.verifyAccount(scope.account.id);
    if (r.status === "ok") scope = { ...scope, account: r.data };
    verifying = false;
  }

  function next() {
    // Fire the profile scrape only on leaving the account step, and only if the
    // user kept online features on — never before they've had the choice.
    if (step === 1 && onlineEnabled) captureAnchor();
    step += 1;
  }
  function back() {
    step = Math.max(0, step - 1);
  }

  async function setOnline(enabled: boolean) {
    if (!settings) return;
    const r = await commands.setOnline(enabled);
    if (r.status === "ok") settings = r.data;
  }

  async function setSensor(enabled: boolean) {
    if (!settings) return;
    const r = await commands.setSensor(enabled);
    if (r.status === "ok") settings = r.data;
    // Enabling kicks off an immediate catch-up scan so the toggle does something
    // now (it also catches up on every launch and tails live during play).
    if (enabled) commands.scanLogsNow();
  }

  async function setLiveSync(enabled: boolean) {
    if (!settings) return;
    const r = await commands.setLiveSync(enabled);
    if (r.status === "ok") settings = r.data;
  }

  function toggleLiveSync() {
    if (!settings) return;
    // First enable needs the one-time ToS consent (shown inline here).
    if (!settings.live_sync_enabled && !settings.live_sync_consented) {
      showConsent = true;
      return;
    }
    const next = !settings.live_sync_enabled;
    setLiveSync(next).then(() => {
      if (next) commands.liveSyncNow(); // first sync; reports via notification
    });
  }

  async function acceptConsent() {
    showConsent = false;
    await setLiveSync(true);
    commands.liveSyncNow();
  }

  async function setLiveInventory(enabled: boolean) {
    if (!settings) return;
    const r = await commands.setLiveInventory(enabled);
    if (r.status === "ok") settings = r.data;
  }

  function toggleLiveInventory() {
    if (!settings) return;
    // Shares the one-time ToS consent with live blueprint sync.
    if (!settings.live_inventory_enabled && !settings.live_sync_consented) {
      showInvConsent = true;
      return;
    }
    const next = !settings.live_inventory_enabled;
    setLiveInventory(next).then(() => {
      if (next) commands.inventorySyncNow(); // first sync; reports via notification
    });
  }

  async function acceptInvConsent() {
    showInvConsent = false;
    await setLiveInventory(true);
    commands.inventorySyncNow();
  }

  const platformLabel = (p: ActiveScope["platform"]) => (p === "prod" ? "PU" : "PTU");
</script>

<div class="overlay">
  <div class="panel">
    {#if step === 0}
      <span class="flame">🔥</span>
      <h1>Welcome to Hearth</h1>
      <p>
        Track your Star Citizen blueprints, missions and wishlist — and keep your
        owned list up to date as you play.
      </p>
    {:else if step === 1}
      <h2>Your account</h2>
      {#if scope}
        <div class="acct">
          <Avatar text={scope.account.handle.charAt(0).toUpperCase()} size="2.4rem" />
          <div class="acct-meta">
            <span class="handle">
              @{scope.account.handle}
              {#if scope.account.last_verified}
                <span class="ok" title="Confirmed via your public RSI profile">✓ #{scope.account.citizen_record}</span>
              {:else if verifying}
                <span class="muted small">verifying…</span>
              {/if}
            </span>
            <span class="muted small">{platformLabel(scope.platform)} · {scope.channel.toUpperCase()}</span>
          </div>
        </div>
        <p class="muted">
          Hearth read this from your RSI launcher. Renamed before, or play on more
          than one account? You can manage that in <strong>Settings → Account</strong>.
        </p>
        {#if settings}
          <div class="opt privacy">
            <div class="opt-head">
              <span class="opt-title">Online features</span>
              <Switch
                checked={onlineEnabled}
                label="Online features"
                onchange={(v) => setOnline(v)}
              />
            </div>
            <p class="muted">
              Lets Hearth read your <strong>public</strong> RSI profile when you
              continue (to confirm this is you and detect handle renames), and
              run live blueprint sync if you turn it on next. Switch off for
              <strong>fully offline</strong> — local game-log tracking still
              works, and you can change this anytime in Settings.
            </p>
          </div>
        {/if}
      {:else if noInstall}
        <p class="muted">
          No Star Citizen install detected yet. Hearth works best with SC installed
          and the launcher signed in — you can still look around, and it'll pick up
          your account once it's there.
        </p>
      {:else if scopeError}
        <p class="muted">
          Couldn't load your account ({scopeError}). Hearth will keep trying in the
          background — restarting usually clears it if it persists.
        </p>
      {:else}
        <p class="muted">Looking for your account…</p>
      {/if}
    {:else}
      <h2>Keeping blueprints up to date</h2>
      <p class="muted intro">
        Pick how Hearth learns which blueprints you own — you can change any of
        this later in Settings.
      </p>
      {#if settings}
        <div class="opt">
          <div class="opt-head">
            <span class="opt-title">Game-log tracking</span>
            <Switch
              checked={settings.sensor_enabled}
              label="Game-log tracking"
              onchange={(v) => setSensor(v)}
            />
          </div>
          <p class="muted">
            Reads your game logs (including past sessions) for blueprints you've
            received and marks them owned — catching up at startup and live while
            you play. Local, ToS-safe — recommended.
          </p>
        </div>

        {#if settings.live_sync_available}
          <div class="opt">
            <div class="opt-head">
              <span class="opt-title">Live blueprint sync <span class="adv">advanced</span></span>
              <Switch
                checked={settings.live_sync_enabled}
                disabled={!onlineEnabled}
                label="Live blueprint sync"
                onchange={() => toggleLiveSync()}
              />
            </div>
            <p class="muted">
              Pulls your <em>complete</em> library straight from your CIG account.
              Unofficial, read-only, <strong>against SC's Terms of Service</strong>
              — your own risk.{#if !onlineEnabled}
                <span class="off-hint"> Turn on Online features above to use this.</span>
              {/if}
            </p>
            {#if showConsent}
              <div class="consent">
                <p class="muted">
                  This connects to CIG's servers with your launcher session to read
                  your blueprints. Read-only and only your own account — but against
                  SC's ToS, at your own risk.
                </p>
                <div class="consent-actions">
                  <button class="btn btn-sm" onclick={() => (showConsent = false)}>Cancel</button>
                  <button class="btn btn-sm btn-primary" onclick={acceptConsent}>I understand — enable</button>
                </div>
              </div>
            {/if}
          </div>

          <div class="opt">
            <div class="opt-head">
              <span class="opt-title">Live resource sync <span class="adv">advanced</span></span>
              <Switch
                checked={settings.live_inventory_enabled}
                disabled={!onlineEnabled}
                label="Live resource sync"
                onchange={() => toggleLiveInventory()}
              />
            </div>
            <p class="muted">
              Reads your in-game resource inventory from your CIG account, so the
              Resources page can show what you have to craft with. Unofficial,
              read-only, <strong>against SC's Terms of Service</strong> — your own
              risk.{#if !onlineEnabled}
                <span class="off-hint"> Turn on Online features above to use this.</span>
              {/if}
            </p>
            {#if showInvConsent}
              <div class="consent">
                <p class="muted">
                  This connects to CIG's servers with your launcher session to read
                  your resource inventory. Read-only and only your own account — but
                  against SC's ToS, at your own risk.
                </p>
                <div class="consent-actions">
                  <button class="btn btn-sm" onclick={() => (showInvConsent = false)}>Cancel</button>
                  <button class="btn btn-sm btn-primary" onclick={acceptInvConsent}>I understand — enable</button>
                </div>
              </div>
            {/if}
          </div>
        {/if}
      {/if}
    {/if}

    <div class="actions">
      {#if step === 0}
        <button class="btn btn-lg" onclick={finishOnboarding}>Skip setup</button>
        <button class="btn btn-lg btn-primary" onclick={next}>Get started</button>
      {:else if step === 1}
        <button class="btn btn-lg" onclick={back}>Back</button>
        <button class="btn btn-lg btn-primary" onclick={next}>Continue</button>
      {:else}
        <button class="btn btn-lg" onclick={back}>Back</button>
        <button class="btn btn-lg btn-primary" onclick={finishOnboarding}>Finish</button>
      {/if}
    </div>

    <div class="dots">
      {#each [0, 1, 2] as d (d)}<span class="dot" class:active={step === d}></span>{/each}
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: grid;
    place-items: safe center;
    overflow-y: auto;
    padding: 2rem;
    background: radial-gradient(
      120% 120% at 50% 0%,
      color-mix(in srgb, var(--ember) 10%, var(--bg)),
      var(--bg) 60%
    );
  }
  .panel {
    width: min(460px, 100%);
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 0.6rem;
  }
  .flame {
    font-size: 2.6rem;
    line-height: 1;
    margin-bottom: 0.2rem;
  }
  h1 {
    margin: 0;
    font-size: 1.6rem;
    letter-spacing: -0.02em;
  }
  h2 {
    margin: 0 0 0.2rem;
    font-size: 1.25rem;
    letter-spacing: -0.01em;
  }
  p {
    margin: 0;
    font-size: 0.92rem;
    color: var(--text);
    line-height: 1.55;
  }
  .muted {
    color: var(--muted);
    font-size: 0.85rem;
  }
  .muted.small {
    font-size: 0.72rem;
  }
  .intro {
    margin-bottom: 0.1rem;
  }
  .consent {
    margin-top: 0.6rem;
    padding-top: 0.6rem;
    border-top: 1px solid var(--line);
  }
  .consent .muted {
    margin: 0 0 0.55rem;
  }
  .consent-actions {
    display: flex;
    gap: 0.5rem;
    justify-content: flex-end;
  }

  .acct {
    display: flex;
    align-items: center;
    gap: 0.7rem;
    padding: 0.7rem 0.9rem;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--panel);
    margin: 0.3rem 0;
  }
  .acct-meta {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.1rem;
  }
  .handle {
    font-weight: 600;
    display: flex;
    align-items: center;
    gap: 0.45rem;
  }
  .ok {
    font-size: 0.72rem;
    color: var(--good);
    font-variant-numeric: tabular-nums;
  }

  .opt {
    width: 100%;
    text-align: left;
    border: 1px solid var(--line);
    border-radius: 10px;
    padding: 0.8rem 0.9rem;
    background: var(--panel);
  }
  .opt-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 0.3rem;
  }
  .opt-title {
    font-weight: 600;
    font-size: 0.92rem;
  }
  .opt .muted {
    margin: 0;
  }

  .actions {
    display: flex;
    gap: 0.7rem;
    margin-top: 0.8rem;
  }
  .off-hint {
    color: var(--ember);
  }

  .dots {
    display: flex;
    gap: 0.4rem;
    margin-top: 1.1rem;
  }
  .dot {
    width: 0.45rem;
    height: 0.45rem;
    border-radius: 50%;
    background: var(--line);
    transition: background 120ms;
  }
  .dot.active {
    background: var(--ember);
  }
</style>
