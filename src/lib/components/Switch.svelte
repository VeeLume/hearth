<script lang="ts">
  // Reusable on/off toggle. Was copy-pasted (identical CSS) into Settings,
  // AccountManager and Onboarding — now one component owns the knob markup,
  // the sliding animation, and the ARIA switch role.
  //
  // Stateless: it renders `checked` and calls `onchange(next)` when clicked.
  // The caller keeps the source of truth and decides what to do with the
  // toggle (which is why these toggles often gate a backend call rather than
  // flipping a local value). Place any text label beside it with `.switch-label`.
  let {
    checked = false,
    disabled = false,
    label,
    onchange,
  }: {
    checked?: boolean;
    disabled?: boolean;
    /** Accessible name (no visible label is rendered). */
    label?: string;
    /** Called with the requested new state when the user clicks. */
    onchange?: (next: boolean) => void;
  } = $props();
</script>

<button
  type="button"
  class="switch"
  class:on={checked}
  {disabled}
  role="switch"
  aria-checked={checked}
  aria-label={label}
  onclick={() => onchange?.(!checked)}
>
  <span class="knob"></span>
</button>

<style>
  .switch {
    width: 2.2rem;
    height: 1.2rem;
    flex: 0 0 auto;
    padding: 0;
    border-radius: 999px;
    border: 1px solid var(--line);
    background: var(--panel-2);
    cursor: pointer;
    position: relative;
    transition: background 120ms, border-color 120ms;
  }
  .switch.on {
    background: var(--ember-glow);
    border-color: var(--ember-dim);
  }
  .knob {
    position: absolute;
    top: 1px;
    left: 1px;
    width: 1rem;
    height: 1rem;
    border-radius: 50%;
    background: var(--muted);
    transition: transform 120ms, background 120ms;
  }
  .switch.on .knob {
    transform: translateX(1rem);
    background: var(--ember);
  }
  .switch:disabled {
    opacity: 0.6;
    cursor: progress;
  }
</style>
