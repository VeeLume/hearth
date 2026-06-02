<script lang="ts">
  import { onMount } from "svelte";
  import { commands, errText, type ActiveScope, type AppSettings } from "$lib/ipc";
  import { finishOnboarding } from "$lib/state/onboardingStore.svelte";
  import { bpImport, runImport } from "$lib/state/importStore.svelte";

  // First-launch flow: Welcome → Your account → Tracking. Lean by design —
  // confirms the account and sets the data source; importing history and live
  // sync live in Settings. Step 0 doubles as cover for the background SC parse.

  let step = $state(0);
  let scope = $state<ActiveScope | null>(null);
  let scopeError = $state<string | null>(null);
  let verifying = $state(false);
  let settings = $state<AppSettings | null>(null);
  let showConsent = $state(false); // inline ToS consent for live sync
  const onlineEnabled = $derived(settings?.online_enabled ?? true);

  onMount(async () => {
    const [s, set] = await Promise.all([commands.activeScope(), commands.getSettings()]);
    if (s.status === "ok") scope = s.data;
    else scopeError = errText(s.error);
    if (set.status === "ok") settings = set.data;
  });

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
          <span class="avatar">{scope.account.handle.charAt(0).toUpperCase()}</span>
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
              <button
                class="switch"
                class:on={onlineEnabled}
                role="switch"
                aria-checked={onlineEnabled}
                aria-label="Online features"
                onclick={() => setOnline(!onlineEnabled)}
              >
                <span class="knob"></span>
              </button>
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
      {:else if scopeError}
        <p class="muted">
          No Star Citizen install detected yet. Hearth works best with SC installed
          and the launcher signed in — you can still look around, and it'll pick up
          your account once it's there.
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
            <span class="opt-title">Live game-log sensing</span>
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
          </div>
          <p class="muted">
            Watches your game log while you play and marks blueprints owned as you
            receive them. Local, ToS-safe — recommended.
          </p>
        </div>

        {#if settings.live_sync_available}
          <div class="opt">
            <div class="opt-head">
              <span class="opt-title">Live blueprint sync <span class="adv">advanced</span></span>
              <button
                class="switch"
                class:on={settings.live_sync_enabled}
                disabled={!onlineEnabled}
                role="switch"
                aria-checked={settings.live_sync_enabled}
                aria-label="Live blueprint sync"
                onclick={toggleLiveSync}
              >
                <span class="knob"></span>
              </button>
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
                  <button class="ghost sm" onclick={() => (showConsent = false)}>Cancel</button>
                  <button class="primary sm" onclick={acceptConsent}>I understand — enable</button>
                </div>
              </div>
            {/if}
          </div>
        {/if}

        <div class="opt">
          <div class="opt-head">
            <span class="opt-title">Import past blueprints</span>
            <button class="ghost sm" onclick={() => runImport()} disabled={bpImport.running}>
              {bpImport.running ? "Importing…" : "Start import"}
            </button>
          </div>
          <p class="muted">
            Scan your game logs for blueprints you've already received and mark them
            owned. Runs in the background — you'll get a notification when it's done.
          </p>
        </div>
      {/if}
    {/if}

    <div class="actions">
      {#if step === 0}
        <button class="ghost" onclick={finishOnboarding}>Skip setup</button>
        <button class="primary" onclick={next}>Get started</button>
      {:else if step === 1}
        <button class="ghost" onclick={back}>Back</button>
        <button class="primary" onclick={next}>Continue</button>
      {:else}
        <button class="ghost" onclick={back}>Back</button>
        <button class="primary" onclick={finishOnboarding}>Finish</button>
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
  .adv {
    font-size: 0.58rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--faint);
    border: 1px solid var(--line);
    border-radius: 4px;
    padding: 0.05rem 0.3rem;
    margin-left: 0.3rem;
    vertical-align: middle;
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
  .avatar {
    width: 2.4rem;
    height: 2.4rem;
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: linear-gradient(135deg, var(--ember), var(--ember-dim));
    color: #1a1209;
    font-weight: 700;
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
  button.primary {
    padding: 0.5rem 1.2rem;
    border-radius: 8px;
    border: 1px solid var(--ember);
    background: var(--ember);
    color: #1a1209;
    font-weight: 600;
    cursor: pointer;
    font-size: 0.9rem;
  }
  button.ghost {
    padding: 0.5rem 1rem;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    font-size: 0.9rem;
  }
  button.ghost:hover {
    color: var(--text);
    border-color: var(--ember-dim);
  }
  /* Small inline action (Start import, consent buttons). */
  button.sm {
    padding: 0.32rem 0.7rem;
    font-size: 0.8rem;
  }
  button.sm:disabled {
    opacity: 0.6;
    cursor: progress;
  }

  /* Toggle switch (matches Settings). */
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
    opacity: 0.5;
    cursor: not-allowed;
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
