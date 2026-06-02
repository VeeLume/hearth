<script lang="ts">
  // Transient toast stack. Watches the notification store and pops a toast for
  // each newly-added notification. Non-sticky levels (info/success) auto-fade;
  // sticky levels (warning/error) stay until dismissed. Dismissing a toast only
  // removes it from this stack — the notification lives on in the center.
  import { onMount } from "svelte";
  import {
    notifications,
    isSticky,
    type Notification,
    type NotifLevel,
  } from "$lib/state/notifications.svelte";

  const AUTO_DISMISS_MS = 6500;

  const glyph: Record<NotifLevel, string> = {
    info: "•",
    success: "✓",
    warning: "!",
    error: "✕",
  };

  type ActiveToast = { n: Notification; timer?: ReturnType<typeof setTimeout> };
  let active = $state<ActiveToast[]>([]);
  const seen = new Set<string>();
  let ready = false;

  onMount(() => {
    // Anything already in the store when we mount predates this session view —
    // don't toast it, just remember it as seen.
    for (const n of notifications.items) seen.add(n.id);
    ready = true;
    return () => {
      for (const t of active) if (t.timer) clearTimeout(t.timer);
    };
  });

  // Detect newly-added notifications and surface each as a toast.
  $effect(() => {
    const list = notifications.items;
    if (!ready) return;
    for (const n of list) {
      if (seen.has(n.id)) continue;
      seen.add(n.id);
      const t: ActiveToast = { n };
      if (!isSticky(n.level)) {
        t.timer = setTimeout(() => remove(n.id), AUTO_DISMISS_MS);
      }
      active = [t, ...active];
    }
  });

  function remove(id: string) {
    const t = active.find((a) => a.n.id === id);
    if (t?.timer) clearTimeout(t.timer);
    active = active.filter((a) => a.n.id !== id);
  }
</script>

{#if active.length > 0}
  <div class="toasts">
    {#each active as t (t.n.id)}
      <div class={`toast ${t.n.level}`} role="status">
        <span class="glyph">{glyph[t.n.level]}</span>
        <div class="body">
          <span class="title">{t.n.title}</span>
          {#if t.n.body}<span class="detail">{t.n.body}</span>{/if}
          {#if t.n.action}
            <a class="action" href={t.n.action.href} onclick={() => remove(t.n.id)}>
              {t.n.action.label} →
            </a>
          {/if}
        </div>
        <button class="close" onclick={() => remove(t.n.id)} aria-label="Dismiss">×</button>
      </div>
    {/each}
  </div>
{/if}

<style>
  .toasts {
    position: fixed;
    bottom: 1.1rem;
    right: 1.1rem;
    z-index: 60;
    display: flex;
    flex-direction: column;
    gap: 0.55rem;
    max-width: 340px;
  }
  .toast {
    display: flex;
    align-items: flex-start;
    gap: 0.6rem;
    padding: 0.7rem 0.8rem;
    background: var(--panel-2);
    border: 1px solid var(--line);
    border-left: 3px solid var(--muted);
    border-radius: 10px;
    box-shadow: 0 8px 28px rgba(0, 0, 0, 0.4);
    animation: toast-in 160ms ease-out;
  }
  .toast.success {
    border-left-color: var(--good);
  }
  .toast.warning {
    border-left-color: var(--warn);
  }
  .toast.error {
    border-left-color: var(--bad);
  }
  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }
  .glyph {
    flex: 0 0 auto;
    width: 1.2rem;
    text-align: center;
    font-weight: 700;
    color: var(--muted);
  }
  .toast.success .glyph {
    color: var(--good);
  }
  .toast.warning .glyph {
    color: var(--warn);
  }
  .toast.error .glyph {
    color: var(--bad);
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    min-width: 0;
  }
  .title {
    font-size: 0.85rem;
    font-weight: 600;
    color: var(--text);
  }
  .detail {
    font-size: 0.74rem;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .action {
    font-size: 0.74rem;
    color: var(--ember);
    text-decoration: none;
    margin-top: 0.1rem;
  }
  .action:hover {
    text-decoration: underline;
  }
  .close {
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
  .close:hover {
    color: var(--text);
  }
</style>
