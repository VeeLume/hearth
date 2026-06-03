<script lang="ts">
  import type { Snippet } from "svelte";

  // The page topbar shared by Catalog, Missions, Wishlist and Settings: a
  // title, an optional subtitle, and an optional right-aligned action area.
  // Each page used to carry an identical copy of these rules.
  //
  // `subtitle` and the default `children` are snippets so callers can pass
  // live markup (counts that change, a search box, a sync button). Anything
  // in `children` is laid out to the right of the title; give it
  // `margin-left: auto` (like the `.search` inputs do) to push it to the edge.
  let {
    title,
    subtitle,
    flush = false,
    children,
  }: {
    title: string;
    subtitle?: Snippet;
    /** Settings sits a tab bar directly under the header: drop the bottom
     *  border and tighten the bottom padding so the two read as one unit. */
    flush?: boolean;
    children?: Snippet;
  } = $props();
</script>

<header class="topbar" class:flush>
  <div class="page-title">
    <h1>{title}</h1>
    {#if subtitle}<span class="subtitle">{@render subtitle()}</span>{/if}
  </div>
  {@render children?.()}
</header>

<style>
  .topbar {
    display: flex;
    align-items: center;
    gap: 1.5rem;
    padding: 1.1rem 1.6rem;
    border-bottom: 1px solid var(--line);
  }
  .topbar.flush {
    padding-bottom: 0.6rem;
    border-bottom: none;
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
    font-variant-numeric: tabular-nums;
  }
</style>
