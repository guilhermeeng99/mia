<script lang="ts">
  // The floating mic HUD pill — dark, translucent, always-on-top (design-system.md
  // §8b). Presentation only: the dictation orchestrator drives `state` + `level`
  // (RMS 0–1) over a Tauri event; this component just renders. The dedicated
  // transparent HUD window (hud.rs + tauri.conf) that mounts it is runtime-pending.
  type HudState = "listening" | "transcribing" | "inserting" | "error";

  interface Props {
    state?: HudState;
    level?: number;
    message?: string;
  }
  let { state = "listening", level = 0, message = "" }: Props = $props();

  // Per-bar multipliers give the waveform an organic shape; height tracks RMS.
  const bars = [0.45, 0.75, 1, 0.7, 0.5];
  const clamped = $derived(Math.min(1, Math.max(0, level)));

  function barHeight(mult: number): number {
    return Math.round(6 + clamped * 22 * mult);
  }

  const label = $derived(
    state === "listening"
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
  {#if state === "listening"}
    <span class="flex items-end gap-1 h-6" aria-hidden="true">
      {#each bars as mult, i (i)}
        <span
          class="w-1 rounded-full bg-hud-wave"
          style="height: {barHeight(mult)}px"
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
