<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { page } from "$app/state";
  import { commands, type ActiveScope } from "$lib/bindings";
  import { primaryNav, futureNav } from "$lib/nav";
  import "../app.css";

  let { children } = $props();

  let scope = $state<ActiveScope | null>(null);
  let scopeError = $state<string | null>(null);
  let verifying = $state(false);
  let verifyMessage = $state<string | null>(null);

  // ── Game.log auto-sensing toast (v1.5) ──────────────────────────────
  // The backend emits `blueprints-sensed` after a poll that auto-marked
  // blueprints owned. Surface it briefly so nothing changes silently.
  type BlueprintsSensed = {
    marked: string[];
    newly_owned: string[];
    unresolved: string[];
  };
  let sensed = $state<BlueprintsSensed | null>(null);
  let sensedTimer: ReturnType<typeof setTimeout> | undefined;
  let unlisten: UnlistenFn | undefined;

  function dismissSensed() {
    sensed = null;
    clearTimeout(sensedTimer);
  }

  onDestroy(() => {
    unlisten?.();
    clearTimeout(sensedTimer);
  });

  onMount(async () => {
    // Auto-sensing toast: only surface passes that actually changed
    // something (newly-owned) or hit an unrecognised name worth flagging.
    unlisten = await listen<BlueprintsSensed>("blueprints-sensed", (event) => {
      const p = event.payload;
      if (p.newly_owned.length === 0 && p.unresolved.length === 0) return;
      sensed = p;
      clearTimeout(sensedTimer);
      sensedTimer = setTimeout(() => (sensed = null), 8000);
    });

    // Remove the pre-hydration splash from app.html now that Svelte
    // is in control. Fades out via the .hidden class for ~200ms.
    const splash = document.getElementById("boot-splash");
    if (splash) {
      splash.classList.add("hidden");
      setTimeout(() => splash.remove(), 250);
    }

    // active_scope now hits only discovery + db (no DCB parse) so this
    // returns in ~50ms even on a cold start, populating the sidebar
    // independently of the catalog page's own onMount.
    const result = await commands.activeScope();
    if (result.status === "ok") {
      scope = result.data;
    } else {
      scopeError = `${result.error.kind}: ${result.error.message}`;
    }
  });

  async function verify() {
    if (!scope) return;
    verifying = true;
    verifyMessage = null;
    const result = await commands.verifyAccount(scope.account.id);
    if (result.status === "ok") {
      scope = { ...scope, account: result.data };
      verifyMessage = `verified · #${result.data.citizen_record}`;
    } else {
      verifyMessage = `${result.error.kind}: ${result.error.message}`;
    }
    verifying = false;
  }

  const platformLabel = (p: ActiveScope["platform"]) => (p === "prod" ? "PU" : "PTU");
  const isActive = (href: string) =>
    href === "/" ? page.url.pathname === "/" : page.url.pathname.startsWith(href);
</script>

<div class="app">
  <aside class="sidebar">
    <div class="brand">
      <span class="flame">🔥</span>
      <span class="brand-name">Hearth</span>
    </div>

    <nav>
      {#each primaryNav as item (item.href)}
        <a class="nav-item" class:active={isActive(item.href)} href={item.href}>
          <span class="nav-icon">{item.icon}</span>
          <span class="nav-label">{item.label}</span>
        </a>
      {/each}

      <div class="nav-divider"></div>

      {#each futureNav as item (item.href)}
        <span class="nav-item disabled">
          <span class="nav-icon">{item.icon}</span>
          <span class="nav-label">{item.label}</span>
          <span class="soon">soon</span>
        </span>
      {/each}
    </nav>

    <div class="account">
      {#if scope}
        <div class="account-handle">
          <span class="avatar">{scope.account.handle.charAt(0).toUpperCase()}</span>
          <div class="account-meta">
            <span class="handle">@{scope.account.handle}</span>
            <span class="scope-line">
              <span class="pf">{platformLabel(scope.platform)}</span>
              <span class="dot">·</span>
              <span class="chan">{scope.channel.toUpperCase()}</span>
              {#if scope.account.last_verified}
                <span class="verified" title={`Verified · #${scope.account.citizen_record}`}>✓</span>
              {/if}
            </span>
          </div>
          <a class="cog" class:active={isActive("/settings")} href="/settings" title="Settings" aria-label="Settings">⚙</a>
        </div>
        <div class="account-actions">
          <button class="verify-btn" onclick={verify} disabled={verifying}>
            {verifying ? "Verifying…" : scope.account.last_verified ? "Re-verify" : "Verify"}
          </button>
          {#if verifyMessage}<span class="verify-msg">{verifyMessage}</span>{/if}
        </div>
      {:else if scopeError}
        <div class="account-handle">
          <span class="avatar err">!</span>
          <div class="account-meta">
            <span class="handle">No account</span>
            <span class="scope-error" title={scopeError}>scope unavailable</span>
          </div>
          <a class="cog" href="/settings" title="Settings" aria-label="Settings">⚙</a>
        </div>
      {:else}
        <div class="account-handle loading">
          <span class="avatar">·</span>
          <div class="account-meta"><span class="handle muted">Loading…</span></div>
        </div>
      {/if}
    </div>
  </aside>

  <main>
    {@render children()}
  </main>
</div>

{#if sensed}
  <div class="toast" role="status">
    <span class="toast-flame">🔥</span>
    <div class="toast-body">
      {#if sensed.newly_owned.length > 0}
        <span class="toast-title">
          Marked {sensed.newly_owned.length} blueprint{sensed.newly_owned.length === 1 ? "" : "s"} owned
        </span>
        <span class="toast-detail">{sensed.marked.slice(0, 4).join(", ")}{sensed.marked.length > 4 ? `, +${sensed.marked.length - 4} more` : ""}</span>
      {:else}
        <span class="toast-title">Detected blueprints from the game</span>
      {/if}
      {#if sensed.unresolved.length > 0}
        <span class="toast-detail muted" title={sensed.unresolved.join("\n")}>
          {sensed.unresolved.length} not recognised in the catalog
        </span>
      {/if}
    </div>
    <button class="toast-close" onclick={dismissSensed} aria-label="Dismiss">×</button>
  </div>
{/if}

<style>
  .app {
    display: grid;
    grid-template-columns: 232px 1fr;
    height: 100vh;
    overflow: hidden;
  }

  .sidebar {
    display: flex;
    flex-direction: column;
    background: var(--panel);
    border-right: 1px solid var(--line);
    padding: 0.75rem 0.6rem;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 0.5rem 1rem;
  }
  .flame {
    font-size: 1.25rem;
  }
  .brand-name {
    font-size: 1.15rem;
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    flex: 1;
  }
  .nav-item {
    display: flex;
    align-items: center;
    gap: 0.65rem;
    padding: 0.5rem 0.65rem;
    border-radius: 7px;
    color: var(--muted);
    text-decoration: none;
    transition: background 90ms, color 90ms;
  }
  a.nav-item:hover {
    background: var(--panel-2);
    color: var(--text);
  }
  .nav-item.active {
    background: var(--ember-glow);
    color: var(--ember);
    font-weight: 500;
  }
  .nav-item.disabled {
    color: var(--faint);
    cursor: default;
  }
  .nav-icon {
    width: 1.1rem;
    text-align: center;
    font-size: 0.95rem;
  }
  .nav-label {
    flex: 1;
    font-size: 0.9rem;
  }
  .soon {
    font-size: 0.62rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--faint);
    border: 1px solid var(--line);
    padding: 0.05rem 0.35rem;
    border-radius: 4px;
  }
  .nav-divider {
    height: 1px;
    background: var(--line);
    margin: 0.6rem 0.5rem;
  }

  .account {
    border-top: 1px solid var(--line);
    padding-top: 0.6rem;
    margin-top: 0.4rem;
  }
  .account-handle {
    display: flex;
    align-items: center;
    gap: 0.55rem;
    padding: 0.35rem 0.5rem;
    border-radius: 7px;
  }
  .avatar {
    width: 1.9rem;
    height: 1.9rem;
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: linear-gradient(135deg, var(--ember), var(--ember-dim));
    color: #1a1209;
    font-weight: 700;
    font-size: 0.85rem;
  }
  .avatar.err {
    background: var(--bad);
    color: #1a1209;
  }
  .account-meta {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .handle {
    font-size: 0.85rem;
    font-weight: 500;
  }
  .handle.muted {
    color: var(--muted);
    font-weight: 400;
  }
  .scope-line {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    font-size: 0.7rem;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .pf {
    color: var(--ember);
    font-weight: 600;
  }
  .dot {
    color: var(--faint);
  }
  .verified {
    color: var(--good);
  }
  .scope-error {
    font-size: 0.7rem;
    color: var(--bad);
    cursor: help;
  }
  .cog {
    margin-left: auto;
    flex: 0 0 auto;
    width: 1.9rem;
    height: 1.9rem;
    display: grid;
    place-items: center;
    border-radius: 7px;
    color: var(--muted);
    text-decoration: none;
    font-size: 1rem;
    transition: background 90ms, color 90ms;
  }
  .cog:hover {
    background: var(--panel-2);
    color: var(--text);
  }
  .cog.active {
    background: var(--ember-glow);
    color: var(--ember);
  }
  .account-actions {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.4rem 0.5rem 0.2rem;
  }
  .verify-btn {
    font-size: 0.7rem;
    padding: 0.2rem 0.55rem;
    background: transparent;
    color: var(--muted);
    border: 1px solid var(--line);
    border-radius: 5px;
    cursor: pointer;
  }
  .verify-btn:hover:not(:disabled) {
    color: var(--text);
    border-color: var(--ember-dim);
  }
  .verify-btn:disabled {
    opacity: 0.5;
    cursor: progress;
  }
  .verify-msg {
    font-size: 0.68rem;
    color: var(--faint);
  }

  main {
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  /* Auto-sensing toast — bottom-right, fades in, auto-dismisses. */
  .toast {
    position: fixed;
    bottom: 1.1rem;
    right: 1.1rem;
    z-index: 50;
    display: flex;
    align-items: flex-start;
    gap: 0.65rem;
    max-width: 340px;
    padding: 0.7rem 0.8rem;
    background: var(--panel-2);
    border: 1px solid var(--ember-dim);
    border-radius: 10px;
    box-shadow: 0 8px 28px rgba(0, 0, 0, 0.4);
    animation: toast-in 160ms ease-out;
  }
  @keyframes toast-in {
    from { opacity: 0; transform: translateY(8px); }
    to { opacity: 1; transform: translateY(0); }
  }
  .toast-flame {
    font-size: 1.1rem;
    line-height: 1.2;
  }
  .toast-body {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    min-width: 0;
  }
  .toast-title {
    font-size: 0.85rem;
    font-weight: 600;
    color: var(--ember);
  }
  .toast-detail {
    font-size: 0.74rem;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .toast-detail.muted {
    color: var(--faint);
    cursor: help;
  }
  .toast-close {
    margin-left: auto;
    flex: 0 0 auto;
    background: transparent;
    border: none;
    color: var(--faint);
    cursor: pointer;
    font-size: 1.1rem;
    line-height: 1;
    padding: 0 0.2rem;
  }
  .toast-close:hover {
    color: var(--text);
  }
</style>
