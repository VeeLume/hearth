<script lang="ts">
  import { onMount } from "svelte";
  import { commands, type AccountWithAliases } from "$lib/bindings";
  import Loading from "$lib/Loading.svelte";

  // Reusable accounts manager — rendered in Settings → Account, and (later) in
  // the first-launch onboarding. RSI handles are mutable (renames); identity is
  // the Hearth account UUID. Here you re-verify an account against its public
  // RSI profile, record former handles, and merge a duplicate a rename created.
  // (Blueprint import from the game logs lives in Settings → Blueprint import.)

  let accounts = $state<AccountWithAliases[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let verifyingId = $state<string | null>(null);

  // Merge form.
  let mergeFrom = $state("");
  let mergeInto = $state("");
  let merging = $state(false);

  // Add-former-handle inline (per account id → draft handle).
  let formerDraft = $state<Record<string, string>>({});

  onMount(load);

  async function load() {
    const res = await commands.listAccountsDetailed();
    if (res.status === "ok") accounts = res.data;
    else error = `${res.error.kind}: ${res.error.message}`;
    loading = false;
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
      error = `${res.error.kind}: ${res.error.message}`;
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
      error = `${res.error.kind}: ${res.error.message}`;
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
      error = `${res.error.kind}: ${res.error.message}`;
    }
  }

</script>

<div class="account-manager">
  {#if error}
    <div class="banner err">{error}</div>
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
                  disabled={verifyingId === a.account.id}
                  title="Look up this handle's public RSI profile to capture/refresh its citizen record"
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
</style>
