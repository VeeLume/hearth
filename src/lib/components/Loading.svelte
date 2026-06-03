<script lang="ts">
  // Shared loading screen: a spinner with a message.
  //
  // Pages that load SC reference data (catalog / missions / wishlist) render
  // this with no props — it predicts the on-disk load path and shows a
  // path-aware message plus a "may take longer" note. Pages with a quick
  // non-SC load (e.g. accounts) pass `message` for a plain labelled spinner.
  import { onMount } from "svelte";
  import { commands, type LoadTier } from "$lib/ipc";

  let { message }: { message?: string } = $props();

  let tier = $state<LoadTier | null>(null);

  onMount(() => {
    if (message) return; // caller supplied its own label — no SC-data load
    commands.predictedLoadTier().then((r) => {
      if (r.status === "ok") tier = r.data;
    });
  });

  function tierMessage(t: LoadTier | null): string {
    switch (t) {
      case "processed":
        return "Loading game data from cache…";
      case "cache":
        return "Rebuilding game data from a snapshot…";
      case "raw":
        return "Reading your Star Citizen game files…";
      default:
        return "Loading…";
    }
  }
</script>

<div class="loading">
  <span class="spinner" aria-hidden="true"></span>
  <p class="loading-msg">{message ?? tierMessage(tier)}</p>
  {#if !message}
    <p class="loading-sub">
      This can take a little longer the first time, or after a Star Citizen update.
    </p>
  {/if}
</div>

<style>
  .loading {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.7rem;
    padding: 3.5rem 1.6rem;
    text-align: center;
  }
  .spinner {
    width: 2rem;
    height: 2rem;
    border-radius: 50%;
    border: 2.5px solid var(--line);
    border-top-color: var(--ember);
    animation: spin 0.8s linear infinite;
  }
  .loading-msg {
    margin: 0;
    font-size: 0.9rem;
    color: var(--text);
  }
  .loading-sub {
    margin: 0;
    font-size: 0.78rem;
    color: var(--faint);
  }
</style>
