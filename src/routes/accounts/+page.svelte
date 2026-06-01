<script lang="ts">
  import { onMount } from "svelte";
  import {
    commands,
    type AccountWithAliases,
    type DiscoveredIdentity,
    type ImportChoice,
    type ImportResult,
  } from "$lib/bindings";

  // Accounts management + Game.log history import.
  //
  // RSI handles are mutable (renames); identity is the Hearth account UUID.
  // This page is where the manual migration lives: record past handles, merge
  // a duplicate account created by a rename, and import blueprint history from
  // the game's session logs — attributing each discovered identity to an
  // account you confirm.

  let accounts = $state<AccountWithAliases[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);

  // Merge form.
  let mergeFrom = $state("");
  let mergeInto = $state("");
  let merging = $state(false);

  // Add-alias inline (per account id → draft handle).
  let aliasDraft = $state<Record<string, string>>({});

  // Import flow.
  let identities = $state<DiscoveredIdentity[] | null>(null);
  let scanning = $state(false);
  let importing = $state(false);
  let importResult = $state<ImportResult | null>(null);
  // key → select value: "__ignore__" | "__new__" | <account id>
  let choice = $state<Record<string, string>>({});

  onMount(load);

  async function load() {
    const res = await commands.listAccountsDetailed();
    if (res.status === "ok") accounts = res.data;
    else error = `${res.error.kind}: ${res.error.message}`;
    loading = false;
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

  async function addAlias(accountId: string) {
    const handle = (aliasDraft[accountId] ?? "").trim();
    if (!handle) return;
    const res = await commands.addAccountAlias(accountId, handle);
    if (res.status === "ok") {
      accounts = res.data;
      aliasDraft[accountId] = "";
    } else {
      error = `${res.error.kind}: ${res.error.message}`;
    }
  }

  async function scan() {
    if (scanning) return;
    scanning = true;
    error = null;
    importResult = null;
    const res = await commands.scanLogHistory();
    if (res.status === "ok") {
      identities = res.data;
      // Default classification: map to the suggested account, else propose a
      // new account (the user scanned in order to import) — switch to ignore
      // per-row as needed.
      const next: Record<string, string> = {};
      for (const id of identities) {
        next[id.key] = id.suggested_account_id ?? "__new__";
      }
      choice = next;
    } else {
      error = `${res.error.kind}: ${res.error.message}`;
    }
    scanning = false;
  }

  async function applyImport() {
    if (!identities || importing) return;
    importing = true;
    error = null;
    const choices: ImportChoice[] = identities.map((id) => {
      const val = choice[id.key] ?? "__ignore__";
      if (val === "__ignore__") return { key: id.key, action: "ignore", account_id: null };
      if (val === "__new__") return { key: id.key, action: "new", account_id: null };
      return { key: id.key, action: "existing", account_id: val };
    });
    const res = await commands.applyLogImport(choices);
    if (res.status === "ok") {
      importResult = res.data;
      identities = null;
      await load(); // new accounts / aliases may have appeared
    } else {
      error = `${res.error.kind}: ${res.error.message}`;
    }
    importing = false;
  }

  const renameChain = (id: DiscoveredIdentity) =>
    id.handles.length > 1 ? id.handles.join(" → ") : (id.handles[0] ?? "unknown");
</script>

<header class="topbar">
  <div class="page-title">
    <h1>Accounts</h1>
    <span class="subtitle">RSI identities, handle renames &amp; history import</span>
  </div>
</header>

<section class="page">
  {#if error}
    <div class="banner err">{error}</div>
  {/if}

  <!-- ── Known accounts ─────────────────────────────────────────── -->
  <div class="card">
    <h2>Known accounts</h2>
    {#if loading}
      <p class="muted">Loading…</p>
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
              </div>
              {#if a.aliases.length}
                <div class="aliases">
                  <span class="aliases-label">also:</span>
                  {#each a.aliases as al (al)}<span class="alias">{al}</span>{/each}
                </div>
              {/if}
              <div class="alias-add">
                <input
                  type="text"
                  placeholder="add a past handle…"
                  bind:value={aliasDraft[a.account.id]}
                  onkeydown={(e) => e.key === "Enter" && addAlias(a.account.id)}
                />
                <button onclick={() => addAlias(a.account.id)}>Add</button>
              </div>
            </div>
          </li>
        {/each}
      </ul>

      {#if accounts.length > 1}
        <div class="merge">
          <h3>Merge accounts</h3>
          <p class="muted">Two rows that are really the same person (e.g. a rename created a duplicate). Owned blueprints &amp; wishlist move to the target; the absorbed handle becomes an alias.</p>
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

  <!-- ── Import from Game.log history ───────────────────────────── -->
  <div class="card">
    <h2>Import from Game.log history</h2>
    <p class="muted">
      Scans your live <code>Game.log</code> + <code>logbackups/</code> for received
      blueprints across past sessions, grouped by RSI account (renames fold
      together via the numeric account id). Prod / persistent-universe sessions
      only — PTU shards wipe. You confirm which identity maps to which account
      before anything is marked owned.
    </p>
    <div class="row">
      <button onclick={scan} disabled={scanning || importing}>
        {scanning ? "Scanning logs…" : "Scan Game.log history"}
      </button>
      {#if importResult}
        <span class="result ok">
          Imported: {importResult.newly_owned} newly owned across {importResult.accounts_touched} account{importResult.accounts_touched === 1 ? "" : "s"}{importResult.unresolved.length ? ` · ${importResult.unresolved.length} unrecognised` : ""}
        </span>
      {/if}
    </div>

    {#if identities}
      {#if identities.length === 0}
        <p class="muted">No blueprint history found in the logs.</p>
      {:else}
        <ul class="ident-list">
          {#each identities as id (id.key)}
            <li class="ident">
              <div class="ident-info">
                <span class="ident-handles">{renameChain(id)}</span>
                <span class="ident-meta">
                  {id.blueprint_count} blueprint{id.blueprint_count === 1 ? "" : "s"} · {id.session_count} session{id.session_count === 1 ? "" : "s"}
                  {#if id.account_hint}· id {id.account_hint}{/if}
                </span>
              </div>
              <select bind:value={choice[id.key]}>
                <option value="__ignore__">Ignore</option>
                <option value="__new__">New account</option>
                {#each accounts as a (a.account.id)}
                  <option value={a.account.id}>→ @{a.account.handle}</option>
                {/each}
              </select>
            </li>
          {/each}
        </ul>
        <div class="row">
          <button class="primary" onclick={applyImport} disabled={importing}>
            {importing ? "Importing…" : "Apply import"}
          </button>
          <span class="muted hint">Owned blueprints are marked in the prod scope; this can't un-own anything.</span>
        </div>
      {/if}
    {/if}
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
  .page-title { display: flex; flex-direction: column; gap: 0.15rem; }
  h1 { margin: 0; font-size: 1.4rem; letter-spacing: -0.02em; }
  .subtitle { font-size: 0.78rem; color: var(--muted); }

  .page { flex: 1; overflow-y: auto; padding: 1rem 1.6rem 2rem; display: flex; flex-direction: column; gap: 1.1rem; }
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
  .muted { color: var(--muted); font-size: 0.83rem; margin: 0.3rem 0; max-width: 70ch; }
  code { background: var(--panel-2); padding: 0.05rem 0.3rem; border-radius: 4px; font-size: 0.9em; }

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
  .aliases { display: flex; align-items: center; gap: 0.35rem; flex-wrap: wrap; font-size: 0.74rem; }
  .aliases-label { color: var(--faint); }
  .alias { color: var(--muted); background: var(--panel-2); padding: 0.05rem 0.4rem; border-radius: 4px; }
  .alias-add { display: flex; gap: 0.4rem; margin-top: 0.1rem; }
  .alias-add input { flex: 0 1 220px; }

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
  button.primary { background: var(--ember); border-color: var(--ember); color: #1a1209; font-weight: 600; }
  button.danger:hover:not(:disabled) { color: var(--bad); border-color: var(--bad); }

  .merge { margin-top: 0.9rem; padding-top: 0.8rem; border-top: 1px solid var(--line); }
  .merge-row { display: flex; align-items: center; gap: 0.5rem; flex-wrap: wrap; }
  .into { color: var(--faint); font-size: 0.8rem; }
  .confirm { font-size: 0.78rem; color: var(--ember); margin: 0.45rem 0 0; }

  .row { display: flex; align-items: center; gap: 0.7rem; flex-wrap: wrap; margin-top: 0.5rem; }
  .result.ok { font-size: 0.8rem; color: var(--good); }
  .hint { font-size: 0.75rem; }

  .ident-list { list-style: none; margin: 0.7rem 0 0; padding: 0; display: flex; flex-direction: column; gap: 0.4rem; }
  .ident {
    display: flex; align-items: center; gap: 0.8rem; padding: 0.5rem 0.6rem;
    border: 1px solid var(--line); border-radius: 8px;
  }
  .ident-info { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 0.1rem; }
  .ident-handles { font-size: 0.88rem; font-weight: 500; }
  .ident-meta { font-size: 0.73rem; color: var(--muted); font-variant-numeric: tabular-nums; }
  .ident select { flex: 0 0 auto; }
</style>
