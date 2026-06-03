<script lang="ts">
  // The circular refresh "sync" control — shared by the catalog (owned-set sync)
  // and the Resources page (inventory sync) so every source's manual refresh
  // looks and behaves identically. Spins while syncing; the `spin` keyframe is
  // global (app.css).
  let {
    onclick,
    syncing = false,
    disabled = false,
    title = "Sync",
    label = "Sync",
  }: {
    onclick: () => void;
    syncing?: boolean;
    disabled?: boolean;
    title?: string;
    label?: string;
  } = $props();
</script>

<button
  class="sync-btn"
  class:syncing
  {onclick}
  disabled={disabled || syncing}
  {title}
  aria-label={label}
>
  <svg
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="2"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <path d="M23 4v6h-6" />
    <path d="M1 20v-6h6" />
    <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" />
  </svg>
</button>

<style>
  .sync-btn {
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    width: 2rem;
    height: 2rem;
    border-radius: 7px;
    background: transparent;
    border: 1px solid var(--line);
    color: var(--muted);
    cursor: pointer;
    transition: color 90ms, border-color 90ms;
  }
  .sync-btn:hover:not(:disabled) {
    color: var(--ember);
    border-color: var(--ember-dim);
  }
  .sync-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .sync-btn.syncing {
    cursor: progress;
  }
  .sync-btn svg {
    width: 1rem;
    height: 1rem;
    display: block;
  }
  .sync-btn.syncing svg {
    animation: spin 0.8s linear infinite;
  }
</style>
