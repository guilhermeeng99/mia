<script lang="ts">
  import type { Phase } from "../dictation";

  // The floating mic HUD pill — "Blush Playground" (design-system.md §8b). A solid
  // white pill with a 2px charcoal outline so it stays legible floating over ANY
  // app (the outline does the separation work — no shadow, Lpalo discipline).
  // Presentation only: the dictation orchestrator drives `state` + `level` (RMS 0–1);
  // `HudWindow.svelte` mounts this in the transparent, click-through HUD window
  // (hud.rs + tauri.conf) and forwards the engine's `hud://state` + `hud://level`.

  interface Props {
    state?: Phase;
    level?: number;
    message?: string;
  }
  let { state = "listening", level = 0, message = "" }: Props = $props();

  // Per-bar multipliers give the waveform an organic shape. The bars pulse via CSS
  // so "listening" is visibly alive, and the live `level` (0–1, forwarded over
  // `hud://level`) scales the heights on top of that.
  const bars = [0.45, 0.75, 1, 0.7, 0.5];
  const clamped = $derived(Math.min(1, Math.max(0, level)));

  function barHeight(mult: number): number {
    return Math.round(10 + (6 + clamped * 16) * mult);
  }

  const label = $derived(
    state === "listening" || state === "idle"
      ? "Ouvindo…"
      : state === "transcribing"
        ? "Transcrevendo…"
        : state === "inserting"
          ? "Inserido"
          : message || "Erro",
  );
</script>

<div
  class="inline-flex select-none items-center gap-3 rounded-pill border-2 border-charcoal
         bg-surface px-4 py-2 text-charcoal"
  data-state={state}
>
  {#if state === "listening" || state === "idle"}
    <span class="flex h-6 items-end gap-1" aria-hidden="true">
      {#each bars as mult, i (i)}
        <span
          class="eq-bar w-1 rounded-pill bg-pumpkin"
          style="height: {barHeight(mult)}px; animation-delay: {i * 0.12}s"
        ></span>
      {/each}
    </span>
  {:else if state === "transcribing"}
    <span
      class="h-4 w-4 animate-spin rounded-full border-2 border-hairline border-t-pumpkin"
      aria-hidden="true"
    ></span>
  {:else if state === "inserting"}
    <span class="text-success" aria-hidden="true">✓</span>
  {:else}
    <span class="text-danger" aria-hidden="true">⚠</span>
  {/if}

  <span class="text-body font-bold">{label}</span>
</div>

<style>
  /* The "listening" bars pulse so the HUD reads as actively recording. Each bar is
     offset (inline animation-delay) for an organic equalizer feel. */
  .eq-bar {
    transform-origin: bottom;
    animation: eq 0.9s ease-in-out infinite;
  }
  @keyframes eq {
    0%,
    100% {
      transform: scaleY(0.4);
    }
    50% {
      transform: scaleY(1);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .eq-bar {
      animation: none;
    }
  }
</style>
