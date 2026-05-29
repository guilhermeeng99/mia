<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import type { DictationEvent } from "./lib/dictation";
  import { installPtt } from "./lib/ptt";
  import { listWhisperModels } from "./lib/stt";
  import Hub from "./lib/components/Hub.svelte";
  import HudWindow from "./lib/components/HudWindow.svelte";
  import Onboarding from "./lib/components/Onboarding.svelte";

  // App.svelte is the single entry for both webviews. The "hud" window renders only
  // the floating mic HUD (driven by the engine's `hud://state` events); the main
  // window renders Settings/Hub + onboarding and wires the global push-to-talk hotkey.
  const isHud = new URLSearchParams(location.search).get("win") === "hud";

  let version = $state("…");
  let showOnboarding = $state(false);

  if (!isHud) {
    invoke<string>("app_version")
      .then((v) => (version = v))
      .catch(() => (version = "n/a"));
  }

  // The hotkey channel still streams session events to the main window, but the HUD
  // is driven by the engine directly (hud://state), so here we only need ptt.ts to
  // run start/stop. Kept as a sink in case the Hub later surfaces live status.
  function onDictationEvent(_e: DictationEvent) {}

  onMount(() => {
    if (isHud) return; // the HUD window manages itself
    listWhisperModels()
      .then((models) => (showOnboarding = !models.some((m) => m.downloaded)))
      .catch(() => {});
    const pending = installPtt(onDictationEvent);
    return () => {
      void pending.then((un) => un());
    };
  });
</script>

{#if isHud}
  <HudWindow />
{:else if showOnboarding}
  <Onboarding ondone={() => (showOnboarding = false)} />
{:else}
  <Hub {version} />
{/if}
