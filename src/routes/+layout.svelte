<script lang="ts">
  let { children } = $props();
</script>

{@render children()}

<style>
  :global(body) {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto,
      "Helvetica Neue", sans-serif;
    background: #131316;
    color: #e8e8ea;
    -webkit-font-smoothing: antialiased;
  }
  :global(*) {
    box-sizing: border-box;
  }
  :global(button) {
    font: inherit;
    color: inherit;
  }
  :global(input) {
    font: inherit;
    color: inherit;
  }
</style>
