<script lang="ts">
  // Shared shell-aware placeholder for views not yet built. Renders the
  // standard topbar + a centered note so unfinished routes still feel
  // like part of the app.
  let { title, icon, note }: { title: string; icon: string; note: string } =
    $props();
</script>

<header class="topbar">
  <div class="page-title">
    <h1>{title}</h1>
    <span class="subtitle">Not built yet</span>
  </div>
</header>

<section class="placeholder">
  <span class="ph-icon">{icon}</span>
  <p class="ph-title">{title}</p>
  <p class="ph-note">{note}</p>
</section>

<style>
  .topbar {
    display: flex;
    align-items: center;
    gap: 1.5rem;
    padding: 1.1rem 1.6rem;
    border-bottom: 1px solid var(--line);
  }
  .page-title {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }
  h1 {
    margin: 0;
    font-size: 1.4rem;
    letter-spacing: -0.02em;
  }
  .subtitle {
    font-size: 0.78rem;
    color: var(--muted);
  }
  .placeholder {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.4rem;
    color: var(--muted);
    padding: 2rem;
  }
  .ph-icon {
    font-size: 2.5rem;
    opacity: 0.5;
  }
  .ph-title {
    font-size: 1.05rem;
    font-weight: 600;
    margin: 0.3rem 0 0;
  }
  .ph-note {
    font-size: 0.85rem;
    color: var(--faint);
    max-width: 360px;
    text-align: center;
    margin: 0;
  }
</style>
