<script lang="ts">
  import type { Snippet } from "svelte";

  // Labeled form row: label + control + optional hint/validation text. `class`
  // lets a caller size the field in a flex/grid row (e.g. `flex-1`).
  interface Props {
    label: string;
    hint?: string;
    class?: string;
    children: Snippet;
  }
  let { label, hint, class: cls = "", children }: Props = $props();
</script>

<!-- The control is wrapped by the <label>, so the label text is programmatically
     associated with it (implicit association) — screen readers announce the input/
     select/textarea with its name, no per-call id wiring needed (design-system.md §9c). -->
<label class="flex flex-col gap-1.5 {cls}">
  <span class="text-body-lg font-bold text-charcoal">{label}</span>
  {@render children()}
  {#if hint}
    <span class="text-body text-ink-soft">{hint}</span>
  {/if}
</label>
