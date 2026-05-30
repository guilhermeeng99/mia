<script lang="ts">
  import type { Snippet } from "svelte";

  // Small status badge — pill shape, 2px charcoal outline (Lpalo). Tone carries
  // meaning via fill + text, never color alone (accessibility, design-system.md).
  interface Props {
    tone?: "neutral" | "success" | "danger" | "accent" | "info";
    children: Snippet;
  }
  let { tone = "neutral", children }: Props = $props();

  const tones: Record<NonNullable<Props["tone"]>, string> = {
    neutral: "bg-surface text-charcoal",
    success: "bg-spring text-charcoal",
    danger: "bg-danger text-surface",
    accent: "bg-pumpkin text-charcoal",
    info: "bg-sky text-charcoal",
  };
</script>

<span
  class="inline-flex items-center gap-1 rounded-pill border-2 border-charcoal
         px-3 py-0.5 text-caption font-bold {tones[tone]}"
>
  {@render children()}
</span>
