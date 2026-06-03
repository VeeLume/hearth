<script lang="ts">
  // The round ember-gradient badge showing an account's initial. Lived as
  // three near-identical copies (sidebar, Onboarding, AccountManager) that
  // differed only in size. Size drives the font size via a CSS variable so
  // the glyph scales with the circle.
  let {
    text,
    size = "1.9rem",
    error = false,
    title,
  }: {
    text: string;
    /** Diameter, any CSS length. The initial is sized at ~45% of it. */
    size?: string;
    /** Red error variant (e.g. the sidebar's "no account" state). */
    error?: boolean;
    title?: string;
  } = $props();
</script>

<span class="avatar" class:err={error} style="--avatar-size: {size}" {title}>{text}</span>

<style>
  .avatar {
    width: var(--avatar-size);
    height: var(--avatar-size);
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: linear-gradient(135deg, var(--ember), var(--ember-dim));
    color: var(--on-ember);
    font-weight: 700;
    font-size: calc(var(--avatar-size) * 0.45);
  }
  .avatar.err {
    background: var(--bad);
  }
</style>
