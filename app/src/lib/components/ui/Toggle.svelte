<script lang="ts">
  // Bindable switch. `checked` is two-way ($bindable); `onchange` fires after a
  // user toggle. ≥40px hit target; reflects state via aria-checked + motion.
  interface Props {
    checked?: boolean;
    label?: string;
    disabled?: boolean;
    onchange?: (checked: boolean) => void;
  }
  let { checked = $bindable(false), label, disabled = false, onchange }: Props = $props();

  function toggle() {
    if (disabled) return;
    checked = !checked;
    onchange?.(checked);
  }
</script>

<button
  type="button"
  role="switch"
  aria-checked={checked}
  {disabled}
  onclick={toggle}
  class="inline-flex items-center gap-3 min-h-[40px] disabled:opacity-50 disabled:cursor-not-allowed"
>
  <span
    class="relative h-6 w-11 rounded-full transition-colors {checked
      ? 'bg-action-blue'
      : 'bg-steel-gray'}"
  >
    <span
      class="absolute top-0.5 left-0.5 h-5 w-5 rounded-full bg-snow-white transition-transform {checked
        ? 'translate-x-5'
        : ''}"
    ></span>
  </span>
  {#if label}
    <span class="text-body-lg text-midnight-indigo">{label}</span>
  {/if}
</button>
