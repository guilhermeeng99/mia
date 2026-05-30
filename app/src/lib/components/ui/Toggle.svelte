<script lang="ts">
  // Bindable switch. `checked` is two-way ($bindable) but the control renders
  // PURELY from the prop — `toggle()` only reports the intended next value via
  // `onchange`; it does NOT optimistically self-flip. That keeps the visual in
  // lockstep with the persisted source: if the parent's save fails and the source
  // value doesn't change, the switch stays put instead of silently desyncing.
  // ≥40px hit target. Lpalo: pill track + 2px charcoal outline, charcoal knob,
  // spring-green fill when on. A visible `label` is required so it's never
  // color-only (accessibility, design-system.md §9c) — it also names the switch.
  interface Props {
    checked?: boolean;
    label: string;
    disabled?: boolean;
    onchange?: (checked: boolean) => void;
  }
  let { checked = $bindable(false), label, disabled = false, onchange }: Props = $props();

  function toggle() {
    if (disabled) return;
    onchange?.(!checked);
  }
</script>

<button
  type="button"
  role="switch"
  aria-checked={checked}
  {disabled}
  onclick={toggle}
  class="inline-flex items-center gap-3 min-h-[40px] outline-none
         focus-visible:ring-4 focus-visible:ring-pumpkin/45 rounded-pill
         disabled:opacity-50 disabled:cursor-not-allowed"
>
  <span
    class="inline-flex h-7 w-12 items-center rounded-pill border-2 border-charcoal px-0.5
           transition-colors {checked ? 'bg-spring' : 'bg-surface'}"
  >
    <span
      class="h-5 w-5 rounded-full bg-charcoal transition-transform {checked
        ? 'translate-x-5'
        : ''}"
    ></span>
  </span>
  <span class="text-body-lg text-charcoal">{label}</span>
</button>
