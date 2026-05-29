<script lang="ts">
  import type { Snippet } from "svelte";

  // Labeled form row: label + control + optional hint/validation text.
  interface Props {
    label: string;
    hint?: string;
    children: Snippet;
  }
  let { label, hint, children }: Props = $props();
</script>

<div class="flex flex-col gap-1.5">
  <span class="text-body-lg font-semibold text-midnight-indigo">{label}</span>
  {@render children()}
  {#if hint}
    <span class="text-body text-slate-blue">{hint}</span>
  {/if}
</div>
