<script lang="ts">
  import type { Snippet } from "svelte";

  // A surface card on the blush canvas (design-system.md). Definition comes from
  // a 2px charcoal outline, NOT a shadow (Lpalo discipline). `tone` swaps the
  // white surface for a vivid accent fill (stat tiles, highlights) and rounds the
  // corner more (the Lpalo "accent card").
  type Tone = "surface" | "pumpkin" | "bubblegum" | "sky" | "seafoam" | "lavender" | "lemon" | "spring";
  interface Props {
    tone?: Tone;
    class?: string;
    children: Snippet;
  }
  let { tone = "surface", class: cls = "", children }: Props = $props();

  const tones: Record<Tone, string> = {
    surface: "bg-surface rounded-card p-6",
    pumpkin: "bg-pumpkin rounded-bubble p-5",
    bubblegum: "bg-bubblegum rounded-bubble p-5 text-surface",
    sky: "bg-sky rounded-bubble p-5",
    seafoam: "bg-seafoam rounded-bubble p-5",
    lavender: "bg-lavender rounded-bubble p-5",
    lemon: "bg-lemon rounded-bubble p-5",
    spring: "bg-spring rounded-bubble p-5",
  };
</script>

<div class="border-2 border-charcoal {tones[tone]} {cls}">
  {@render children()}
</div>
