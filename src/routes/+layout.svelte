<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { page } from "$app/state";
  import {
    commands,
    errText,
    onActiveScopeChanged,
    type ActiveScope,
    type UnlistenFn,
  } from "$lib/ipc";
  import { primaryNav, futureNav } from "$lib/nav";
  import Toasts from "$lib/components/Toasts.svelte";
  import NotificationCenter from "$lib/components/NotificationCenter.svelte";
  import { notifications, listenForNotifications, markAllRead } from "$lib/state/notifications.svelte";
  import {
    ensureBlueprints,
    ensureOwnership,
    ensureMissions,
    ensureGrantedBy,
    listenForOwnershipChanges,
  } from "$lib/state/data.svelte";
  import Onboarding from "$lib/components/Onboarding.svelte";
  import { onboarding, maybeStart } from "$lib/state/onboardingStore.svelte";
  import { maybeStartupImport } from "$lib/state/importStore.svelte";
  import { checkForUpdates } from "$lib/updater";
  import "../app.css";

  let { children } = $props();

  let scope = $state<ActiveScope | null>(null);
  let scopeError = $state<string | null>(null);

  // Notification center (sidebar bell). Opening it marks everything read so
  // the bell badge clears; the per-session log stays in the panel.
  let centerOpen = $state(false);
  let unlisten: UnlistenFn | undefined;
  let unlistenOwnership: UnlistenFn | undefined;
  let unlistenScope: UnlistenFn | undefined;

  function toggleCenter() {
    centerOpen = !centerOpen;
    if (centerOpen) markAllRead();
  }

  async function loadScope() {
    const result = await commands.activeScope();
    if (result.status === "ok") {
      scope = result.data;
      scopeError = null;
    } else {
      scopeError = errText(result.error);
    }
  }

  onDestroy(() => {
    unlisten?.();
    unlistenOwnership?.();
    unlistenScope?.();
  });

  onMount(async () => {
    // The single funnel: every backend `notify` event lands in the store,
    // which drives both the toast stack and the notification center.
    unlisten = await listenForNotifications();
    // Refresh the shared owned set whenever the backend changes it behind our
    // back (live sync reconcile, sensor auto-mark).
    unlistenOwnership = await listenForOwnershipChanges();
    // Re-read the active scope when it changes behind our back — e.g. the
    // startup rename check auto-applied a handle rename, swapping which account
    // row is active.
    unlistenScope = await onActiveScopeChanged(loadScope);

    // Warm the shared data store in the background (one backend load serves
    // all of these) so every page renders instantly when reached — no
    // per-page fetch or loading flash, even on the first visit.
    ensureBlueprints();
    ensureOwnership();
    ensureMissions();
    ensureGrantedBy();

    // Show first-launch onboarding if it hasn't been completed.
    maybeStart();

    // If sensing is on, quietly catch up on blueprints logged while the app
    // was closed (now rotated into logbackups/). Fast via the per-file cache.
    maybeStartupImport();

    // Check GitHub for a newer release (skipped in offline mode). Fire-and-forget.
    checkForUpdates();

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
    await loadScope();
  });


  const platformLabel = (p: ActiveScope["platform"]) => (p === "prod" ? "PU" : "PTU");
  const isActive = (href: string) =>
    href === "/" ? page.url.pathname === "/" : page.url.pathname.startsWith(href);
</script>

<div class="app">
  <aside class="sidebar">
    <div class="brand">
      <span class="flame">🔥</span>
      <span class="brand-name">Hearth</span>
      <button
        class="bell"
        class:open={centerOpen}
        onclick={toggleCenter}
        title="Notifications"
        aria-label="Notifications"
      >
        <svg
          class="bell-icon"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="1.8"
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9" />
          <path d="M13.73 21a2 2 0 0 1-3.46 0" />
        </svg>
        {#if notifications.unread > 0}
          <span class="bell-badge">{notifications.unread > 9 ? "9+" : notifications.unread}</span>
        {/if}
      </button>
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

<NotificationCenter open={centerOpen} onClose={() => (centerOpen = false)} />
<Toasts />

{#if onboarding.open}
  <Onboarding />
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
  main {
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  /* Notification bell — right of the brand, badge shows unread count. */
  /* Matches the catalog's ⚐/♡ wish toggles: faint outline glyph, muted on
     hover, ember when the center is open. Deliberately low-key — the unread
     badge carries the emphasis, not the bell. */
  .bell {
    position: relative;
    margin-left: auto;
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    padding: 0.25rem 0.3rem;
    background: transparent;
    border: none;
    cursor: pointer;
    color: var(--faint);
    transition: color 90ms, transform 90ms;
  }
  .bell-icon {
    display: block;
    width: 1.05rem;
    height: 1.05rem;
  }
  .bell:hover {
    color: var(--muted);
    transform: scale(1.12);
  }
  .bell.open {
    color: var(--ember);
  }
  .bell-badge {
    position: absolute;
    top: -0.1rem;
    right: -0.1rem;
    min-width: 1rem;
    height: 1rem;
    padding: 0 0.2rem;
    display: grid;
    place-items: center;
    border-radius: 999px;
    background: var(--ember);
    color: #1a1209;
    font-size: 0.6rem;
    font-weight: 700;
    line-height: 1;
  }
</style>
