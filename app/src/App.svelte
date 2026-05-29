<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import type { DictationEvent } from "./lib/dictation";
  import { installPtt } from "./lib/ptt";
  import Hub from "./lib/components/Hub.svelte";
  import MicHud from "./lib/components/MicHud.svelte";

  // App.svelte stays thin: resolve the version (IPC smoke test), wire the global
  // push-to-talk hotkey, render the Settings/Hub + the floating mic HUD overlay.
  type Phase = "idle" | "listening" | "transcribing" | "inserting" | "error";

  let version = $state("…");
  let phase = $state<Phase>("idle");
  let hudMsg = $state("");

  invoke<string>("app_version")
    .then((v) => (version = v))
    .catch(() => (version = "n/a"));

  function onDictationEvent(e: DictationEvent) {
    if (e.kind === "stateChanged") {
      phase = e.phase;
    } else if (e.kind === "error") {
      phase = "error";
      hudMsg = e.message;
    } else if (e.kind === "cancelled" || e.kind === "injected") {
      phase = "idle";
    }
  }

  onMount(() => {
    const pending = installPtt(onDictationEvent);
    return () => {
      void pending.then((un) => un());
    };
  });
</script>

<Hub {version} />

{#if phase !== "idle"}
  <div class="fixed bottom-6 left-1/2 z-50 -translate-x-1/2">
    <MicHud state={phase} message={hudMsg} />
  </div>
{/if}
