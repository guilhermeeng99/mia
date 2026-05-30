<script lang="ts">
  import type { Snippet } from "svelte";

  // The header at the top of each Hub view: a bold Alfa Slab One title (Lpalo
  // display) + optional helper line, with an optional right-aligned action slot.
  interface Props {
    title: string;
    subtitle?: string;
    action?: Snippet;
  }
  let { title, subtitle, action }: Props = $props();
</script>

<header class="mb-7 flex items-end justify-between gap-4">
  <div>
    <h1 class="font-display text-page text-charcoal">{title}</h1>
    {#if subtitle}
      <p class="mt-1 text-body-lg text-ink-soft">{subtitle}</p>
    {/if}
  </div>
  {#if action}
    <div class="shrink-0">{@render action()}</div>
  {/if}
</header>
