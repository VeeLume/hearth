<script lang="ts">
  import { onMount } from "svelte";
  import { commands, errText, type AccountWithAliases, type AppSettings } from "$lib/ipc";
  import Loading from "$lib/components/Loading.svelte";

  // Reusable accounts manager — rendered in Settings → Account, and (later) in
  // the first-launch onboarding. RSI handles are mutable (renames); identity is
  // the Hearth account UUID. Here you re-verify an account against its public
  // RSI profile, record former handles, and merge a duplicate a rename created.
  // (Blueprint import from the game logs lives in Settings → Blueprint import.)

  let accounts = $state<AccountWithAliases[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let verifyingId = $state<string | null>(null);

  // Master online switch. When off, Hearth makes no network calls at all —
  // profile lookups (verify buttons disabled here) and live blueprint sync.
  let settings = $state<AppSettings | null>(null);
  let togglingOnline = $state(false);
  const onlineEnabled = $derived(settings?.online_enabled ?? true);

  // Merge form.
  let mergeFrom = $state("");
  let mergeInto = $state("");
  let merging = $state(false);

  // Add-former-handle inline (per account id → draft handle).
  let formerDraft = $state<Record<string, string>>({});

  onMount(load);

  async function load() {
    const [res, set] = await Promise.all([
      commands.listAccountsDetailed(),
      commands.getSettings(),
    ]);
    if (res.status === "ok") accounts = res.data;
    else error = errText(res.error);
    if (set.status === "ok") settings = set.data;
    loading = false;
  }

  async function setOnline(enabled: boolean) {
    if (togglingOnline) return;
    togglingOnline = true;
    const res = await commands.setOnline(enabled);
    if (res.status === "ok") settings = res.data;
    else error = errText(res.error);
    togglingOnline = false;
  }

  /** Scrape the account's public RSI profile to capture/refresh its immutable
   *  anchors (citizen record, enlisted) and last-verified timestamp. */
  async function reverify(accountId: string) {
    if (verifyingId) return;
    verifyingId = accountId;
    error = null;
    const res = await commands.verifyAccount(accountId);
    if (res.status === "ok") {
      accounts = accounts.map((a) =>
        a.account.id === accountId ? { ...a, account: res.data } : a,
      );
    } else {
      error = errText(res.error);
    }
    verifyingId = null;
  }

  function handleOf(id: string): string {
    return accounts.find((a) => a.account.id === id)?.account.handle ?? id;
  }

  async function doMerge() {
    if (!mergeFrom || !mergeInto || mergeFrom === mergeInto || merging) return;
    merging = true;
    error = null;
    const res = await commands.mergeAccounts(mergeFrom, mergeInto);
    if (res.status === "ok") {
      accounts = res.data;
      mergeFrom = "";
      mergeInto = "";
    } else {
      error = errText(res.error);
    }
    merging = false;
  }

  async function addFormerHandle(accountId: string) {
    const handle = (formerDraft[accountId] ?? "").trim();
    if (!handle) return;
    const res = await commands.addAccountAlias(accountId, handle);
    if (res.status === "ok") {
      accounts = res.data;
      formerDraft[accountId] = "";
    } else {
      error = errText(res.error);
    }
  }

</script>

<div class="account-manager">
  {#if error}
    <div class="banner err">{error}</div>
  {/if}

  <!-- ── Online features (master switch / offline mode) ─────────────── -->
  {#if settings}
    <div class="card">
      <h2>Online features <span class="adv">privacy</span></h2>
      <p class="muted">
        Controls whether Hearth contacts the network at all. When on, it reads
        your <strong>public</strong> RSI profile (the page anyone can view) to
        confirm your identity and detect handle renames, and — if you've enabled
        it — runs <strong>live blueprint sync</strong>. Turn this off for
        <strong>fully offline</strong>: no profile lookups and no live sync, ever.
        Local game-log tracking and manual editing still work.
      </p>
      <div class="lookups-row">
        <button
          class="switch"
          class:on={onlineEnabled}
          disabled={togglingOnline}
          role="switch"
          aria-checked={onlineEnabled}
          aria-label="Online features"
          onclick={() => setOnline(!onlineEnabled)}
        >
          <span class="knob"></span>
        </button>
        <span class="switch-label">{onlineEnabled ? "Online" : "Offline"}</span>
      </div>
    </div>
  {/if}

  <!-- ── Known accounts ─────────────────────────────────────────── -->
  <div class="card">
    <h2>Known accounts</h2>
    {#if loading}
      <Loading message="Loading accounts…" />
    {:else if accounts.length === 0}
      <p class="muted">No accounts yet — they're created from your launcher handle on first use.</p>
    {:else}
      <ul class="acct-list">
        {#each accounts as a (a.account.id)}
          <li class="acct">
            <span class="avatar">{a.account.handle.charAt(0).toUpperCase()}</span>
            <div class="acct-body">
              <div class="acct-head">
                <span class="acct-handle">@{a.account.handle}</span>
                {#if a.account.citizen_record}
                  <span class="badge ok" title="Verified · #{a.account.citizen_record}">✓ #{a.account.citizen_record}</span>
                {/if}
                {#if a.account.account_hint}
                  <span class="badge dim" title="Numeric accountId from the launcher / logs">id {a.account.account_hint}</span>
                {/if}
                <button
                  class="reverify"
                  onclick={() => reverify(a.account.id)}
                  disabled={verifyingId === a.account.id || !onlineEnabled}
                  title={onlineEnabled
                    ? "Look up this handle's public RSI profile to capture/refresh its citizen record"
                    : "Hearth is in offline mode (see Online features above)"}
                >
                  {verifyingId === a.account.id ? "Verifying…" : a.account.last_verified ? "Re-verify" : "Verify"}
                </button>
              </div>
              {#if a.aliases.length}
                <div class="formers">
                  <span class="formers-label">Previously:</span>
                  {#each a.aliases as h (h)}<span class="former">@{h}</span>{/each}
                </div>
              {/if}
              <div class="former-add">
                <input
                  type="text"
                  placeholder="add a former handle…"
                  bind:value={formerDraft[a.account.id]}
                  onkeydown={(e) => e.key === "Enter" && addFormerHandle(a.account.id)}
                />
                <button onclick={() => addFormerHandle(a.account.id)}>Add</button>
              </div>
            </div>
          </li>
        {/each}
      </ul>

      {#if accounts.length > 1}
        <div class="merge">
          <h3>Merge accounts</h3>
          <p class="muted">Two rows that are really the same account (e.g. a rename created a duplicate). Owned blueprints &amp; wishlist move to the target; the absorbed handle becomes a former handle.</p>
          <div class="merge-row">
            <select bind:value={mergeFrom}>
              <option value="">— absorb —</option>
              {#each accounts as a (a.account.id)}<option value={a.account.id}>@{a.account.handle}</option>{/each}
            </select>
            <span class="into">into</span>
            <select bind:value={mergeInto}>
              <option value="">— keep —</option>
              {#each accounts as a (a.account.id)}<option value={a.account.id}>@{a.account.handle}</option>{/each}
            </select>
            <button
              class="danger"
              disabled={!mergeFrom || !mergeInto || mergeFrom === mergeInto || merging}
              onclick={doMerge}
            >{merging ? "Merging…" : "Merge"}</button>
          </div>
          {#if mergeFrom && mergeInto && mergeFrom !== mergeInto}
            <p class="confirm">@{handleOf(mergeFrom)} will be absorbed into @{handleOf(mergeInto)} and removed.</p>
          {/if}
        </div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .account-manager {
    display: flex;
    flex-direction: column;
    gap: 1.1rem;
  }
  .banner.err {
    padding: 0.6rem 0.85rem;
    border-radius: 8px;
    background: color-mix(in srgb, var(--bad) 14%, transparent);
    border: 1px solid var(--bad);
    color: var(--bad);
    font-size: 0.82rem;
  }
  .card {
    border: 1px solid var(--line);
    border-radius: 10px;
    padding: 1rem 1.1rem;
    background: var(--panel);
  }
  .card h2 { margin: 0 0 0.5rem; font-size: 1rem; }
  .card h3 { margin: 0 0 0.3rem; font-size: 0.85rem; }
  .muted { color: var(--muted); font-size: 0.83rem; margin: 0.3rem 0; }

  .acct-list { list-style: none; margin: 0.4rem 0 0; padding: 0; display: flex; flex-direction: column; gap: 0.6rem; }
  .acct { display: flex; gap: 0.7rem; padding: 0.6rem; border: 1px solid var(--line); border-radius: 8px; }
  .avatar {
    width: 2rem; height: 2rem; flex: 0 0 auto; display: grid; place-items: center;
    border-radius: 50%; background: linear-gradient(135deg, var(--ember), var(--ember-dim));
    color: #1a1209; font-weight: 700; font-size: 0.85rem;
  }
  .acct-body { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 0.35rem; }
  .acct-head { display: flex; align-items: center; gap: 0.5rem; flex-wrap: wrap; }
  .acct-handle { font-weight: 600; font-size: 0.92rem; }
  .badge { font-size: 0.68rem; padding: 0.08rem 0.4rem; border-radius: 999px; border: 1px solid var(--line); }
  .badge.ok { color: var(--good); border-color: var(--good); }
  .badge.dim { color: var(--faint); font-variant-numeric: tabular-nums; }
  .reverify { margin-left: auto; }
  .formers { display: flex; align-items: center; gap: 0.35rem; flex-wrap: wrap; font-size: 0.74rem; }
  .formers-label { color: var(--faint); }
  .former { color: var(--muted); background: var(--panel-2); padding: 0.05rem 0.4rem; border-radius: 4px; }
  .former-add { display: flex; gap: 0.4rem; margin-top: 0.1rem; }
  .former-add input { flex: 0 1 220px; }

  input, select {
    background: var(--panel-2); color: var(--text);
    border: 1px solid var(--line); border-radius: 6px;
    padding: 0.3rem 0.5rem; font-size: 0.82rem;
  }
  button {
    padding: 0.32rem 0.7rem; border-radius: 6px; border: 1px solid var(--line);
    background: transparent; color: var(--muted); cursor: pointer; font-size: 0.82rem;
  }
  button:hover:not(:disabled) { color: var(--text); border-color: var(--ember-dim); }
  button:disabled { opacity: 0.5; cursor: default; }
  button.danger:hover:not(:disabled) { color: var(--bad); border-color: var(--bad); }

  .merge { margin-top: 0.9rem; padding-top: 0.8rem; border-top: 1px solid var(--line); }
  .merge-row { display: flex; align-items: center; gap: 0.5rem; flex-wrap: wrap; }
  .into { color: var(--faint); font-size: 0.8rem; }
  .confirm { font-size: 0.78rem; color: var(--ember); margin: 0.45rem 0 0; }

  /* ── Privacy toggle ── */
  .adv {
    font-size: 0.6rem; text-transform: uppercase; letter-spacing: 0.06em;
    color: var(--faint); border: 1px solid var(--line); border-radius: 4px;
    padding: 0.05rem 0.35rem; margin-left: 0.4rem; vertical-align: middle;
  }
  .lookups-row { display: flex; align-items: center; gap: 0.7rem; margin-top: 0.7rem; }
  .switch {
    width: 2.2rem; height: 1.2rem; flex: 0 0 auto; padding: 0;
    border-radius: 999px; border: 1px solid var(--line); background: var(--panel-2);
    cursor: pointer; position: relative; transition: background 120ms, border-color 120ms;
  }
  .switch.on { background: var(--ember-glow); border-color: var(--ember-dim); }
  .switch .knob {
    position: absolute; top: 1px; left: 1px; width: 1rem; height: 1rem;
    border-radius: 50%; background: var(--muted); transition: transform 120ms, background 120ms;
  }
  .switch.on .knob { transform: translateX(1rem); background: var(--ember); }
  .switch:disabled { opacity: 0.6; cursor: progress; }
  .switch-label { font-size: 0.82rem; color: var(--muted); }
</style>
