<script lang="ts">
  import { onMount } from "svelte";
  import { appVersion } from "./lib/app";
  import type { DictationEvent } from "./lib/dictation";
  import { installPtt } from "./lib/ptt";
  import { getSettings, updateSettings, type GeneralSettings } from "./lib/settings";
  import { listWhisperModels } from "./lib/stt";
  import Hub from "./lib/components/Hub.svelte";
  import HudWindow from "./lib/components/HudWindow.svelte";
  import Onboarding from "./lib/components/Onboarding.svelte";
  import WindowTitleBar from "./lib/components/WindowTitleBar.svelte";

  // App.svelte is the single entry for both webviews. The "hud" window renders only
  // the floating mic HUD (driven by the engine's `hud://state` events); the main
  // window renders Settings/Hub + onboarding and wires the global push-to-talk hotkey.
  const isHud = new URLSearchParams(location.search).get("win") === "hud";

  let version = $state("…");
  let booted = $state(isHud);
  let showOnboarding = $state(false);
  let general = $state<GeneralSettings | null>(null);

  if (!isHud) {
    appVersion()
      .then((v) => (version = v))
      .catch(() => (version = "n/a"));
  }

  // Finish onboarding: persist the flag so MIA boots to the tray next time, then
  // close the wizard (onboarding.md Rule 1/14).
  async function finishOnboarding() {
    if (!general) {
      showOnboarding = false;
      return;
    }
    try {
      const s = await updateSettings({ general: { ...general, onboardingCompleted: true } });
      general = s.general;
    } catch {
      /* a persistence hiccup must not trap the user in the wizard */
    } finally {
      showOnboarding = false;
    }
  }

  // The hotkey channel still streams session events to the main window, but the HUD
  // is driven by the engine directly (hud://state), so here we only need ptt.ts to
  // run start/stop. Kept as a sink in case the Hub later surfaces live status.
  function onDictationEvent(_e: DictationEvent) {}

  onMount(() => {
    if (isHud) return; // the HUD window manages itself
    // Show the wizard only until it's been completed once. A pre-existing install
    // (a model already on disk) is treated as completed so it boots straight to the
    // Hub instead of re-prompting (onboarding.md Rule 1).
    async function boot() {
      try {
        const [s, models] = await Promise.all([getSettings(), listWhisperModels()]);
        general = s.general;
        const activeModelDownloaded = models.some((m) => m.id === s.model.model && m.downloaded);
        showOnboarding = !s.general.onboardingCompleted && !activeModelDownloaded;
        if (!s.general.onboardingCompleted && activeModelDownloaded) {
          try {
            const next = await updateSettings({
              general: { ...s.general, onboardingCompleted: true },
            });
            general = next.general;
          } catch {
            /* best-effort repair for existing installs */
          }
        }
      } catch {
        showOnboarding = false;
      } finally {
        booted = true;
      }
    }
    void boot();
    const pending = installPtt(onDictationEvent);
    return () => {
      void pending.then((un) => un());
    };
  });
</script>

{#if isHud}
  <HudWindow />
{:else if !booted}
  <div class="flex h-screen flex-col overflow-hidden bg-canvas">
    <WindowTitleBar />
    <div class="min-h-0 flex-1"></div>
  </div>
{:else if showOnboarding}
  <div class="flex h-screen flex-col overflow-hidden bg-canvas">
    <WindowTitleBar />
    <div class="min-h-0 flex-1">
      <Onboarding ondone={finishOnboarding} />
    </div>
  </div>
{:else}
  <div class="flex h-screen flex-col overflow-hidden bg-canvas">
    <WindowTitleBar />
    <div class="min-h-0 flex-1">
      <Hub {version} />
    </div>
  </div>
{/if}
