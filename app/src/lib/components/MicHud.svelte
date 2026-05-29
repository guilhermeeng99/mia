<script lang="ts">
  // The floating mic HUD pill — dark, translucent, always-on-top (design-system.md
  // §8b). Presentation only: the dictation orchestrator drives `state` + `level`
  // (RMS 0–1) over a Tauri event; this component just renders. The dedicated
  // transparent HUD window (hud.rs + tauri.conf) that mounts it is runtime-pending.
  type HudState = "idle" | "listening" | "transcribing" | "inserting" | "error";

  interface Props {
    state?: HudState;
    level?: number;
    message?: string;
  }
  let { state = "listening", level = 0, message = "" }: Props = $props();

  // Per-bar multipliers give the waveform an organic shape. Until live RMS
  // forwarding lands, the bars pulse via CSS so "listening" is visibly alive; when
  // `level` (0–1) arrives it scales the heights on top of that.
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
  class="inline-flex items-center gap-3 rounded-full border border-hud-border bg-hud-bg
         px-4 py-2 text-hud-text shadow-hud select-none"
  data-state={state}
>
  {#if state === "listening" || state === "idle"}
    <span class="flex items-end gap-1 h-6" aria-hidden="true">
      {#each bars as mult, i (i)}
        <span
          class="eq-bar w-1 rounded-full bg-hud-wave"
          style="height: {barHeight(mult)}px; animation-delay: {i * 0.12}s"
        ></span>
      {/each}
    </span>
  {:else if state === "transcribing"}
    <span
      class="h-4 w-4 animate-spin rounded-full border-2 border-hud-text-dim border-t-hud-accent"
      aria-hidden="true"
    ></span>
  {:else if state === "inserting"}
    <span class="text-hud-success" aria-hidden="true">✓</span>
  {:else}
    <span class="text-hud-danger" aria-hidden="true">⚠</span>
  {/if}

  <span class="text-body font-semibold">{label}</span>
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
