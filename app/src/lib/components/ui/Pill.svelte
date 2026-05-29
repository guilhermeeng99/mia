<script lang="ts">
  import type { Snippet } from "svelte";

  // Small status pill. Tone carries meaning via text + icon, never color alone
  // (accessibility discipline, design-system.md).
  interface Props {
    tone?: "neutral" | "success" | "danger" | "action";
    children: Snippet;
  }
  let { tone = "neutral", children }: Props = $props();

  const tones: Record<NonNullable<Props["tone"]>, string> = {
    neutral: "text-slate-blue",
    success: "text-success",
    danger: "text-danger",
    action: "text-action-blue",
  };
</script>

<span
  class="inline-flex items-center gap-1 rounded-full border border-platinum-tint bg-cloud-mist
         px-2 py-1 text-body font-semibold {tones[tone]}"
>
  {@render children()}
</span>
