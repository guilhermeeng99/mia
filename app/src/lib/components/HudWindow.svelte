<script lang="ts">
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import type { Phase } from "../dictation";
  import MicHud from "./MicHud.svelte";

  // The floating HUD webview. Listens to the engine's `hud://state` events and shows
  // the pill; renders nothing when idle so the transparent, click-through window
  // (hud.rs) is invisible. The Error phase is latched briefly so a failure is
  // readable even though the engine returns to Idle immediately after it.
  let phase = $state<Phase>("idle");
  let message = $state("");
  let errorTimer: ReturnType<typeof setTimeout> | undefined;

  function onState(next: Phase, msg: string) {
    if (next === "error") {
      phase = "error";
      message = msg;
      clearTimeout(errorTimer);
      errorTimer = setTimeout(() => (phase = "idle"), 3000);
      return;
    }
    // Hold the error on screen until its timer fires (the engine idles right after).
    if (next === "idle" && phase === "error") return;
    clearTimeout(errorTimer);
    phase = next;
  }

  onMount(() => {
    document.body.classList.add("hud");
    const pending: Promise<UnlistenFn> = listen<{ phase: Phase; message: string | null }>(
      "hud://state",
      ({ payload }) => onState(payload.phase, payload.message ?? ""),
    );
    return () => void pending.then((un) => un());
  });
</script>

{#if phase !== "idle"}
  <div class="grid min-h-screen place-items-center">
    <MicHud state={phase} {message} />
  </div>
{/if}
