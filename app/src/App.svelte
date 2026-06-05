<script lang="ts">
  import { onMount } from "svelte";
  import { isTauri } from "@tauri-apps/api/core";
  import { appVersion } from "./lib/app";
  import type { DictationEvent } from "./lib/dictation";
  import { installPtt } from "./lib/ptt";
  import { setUiLanguagePreference } from "./lib/i18n";
  import { getSettings, updateSettings, type GeneralSettings } from "./lib/settings";
  import { listWhisperModels } from "./lib/stt";
  import Hub from "./lib/components/Hub.svelte";
  import HudWindow from "./lib/components/HudWindow.svelte";
  import Onboarding from "./lib/components/Onboarding.svelte";
  import WindowTitleBar from "./lib/components/WindowTitleBar.svelte";

  // App.svelte is the single entry for both webviews. The "hud" window renders only
  // the floating mic HUD (driven by the engine's `hud://state` events); the main
  // window renders Settings/Hub + onboarding and wires the global push-to-talk hotkey.
  const isDesktopRuntime = isTauri();
  const isHud = isDesktopRuntime && new URLSearchParams(location.search).get("win") === "hud";

  let version = $state("…");
  let booted = $state(isHud);
  let showOnboarding = $state(false);
  let general = $state<GeneralSettings | null>(null);

  if (isDesktopRuntime && !isHud) {
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
    if (!isDesktopRuntime) return;
    if (isHud) {
      getSettings().then((s) => setUiLanguagePreference(s.general.uiLanguage)).catch(() => {});
      return; // the HUD window manages itself
    }
    // Show the wizard only until it's been completed once. A pre-existing install
    // (a model already on disk) is treated as completed so it boots straight to the
    // Hub instead of re-prompting (onboarding.md Rule 1).
    async function boot() {
      try {
        const [s, models] = await Promise.all([getSettings(), listWhisperModels()]);
        setUiLanguagePreference(s.general.uiLanguage);
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

{#if !isDesktopRuntime}
  <div class="flex min-h-screen items-center justify-center bg-canvas px-6 py-12 font-body text-charcoal">
    <main class="max-w-[760px] text-center">
      <img src="/logo.png" alt="MIA" class="mx-auto h-16 w-auto" />
      <span class="mt-8 inline-flex rounded-pill border-2 border-charcoal bg-lemon px-4 py-1 text-body font-bold">
        Desktop app only
      </span>
      <h1 class="mt-6 font-display text-display-sm md:text-display">
        MIA runs as a Windows app
      </h1>
      <p class="mx-auto mt-4 max-w-2xl text-subheading text-ink-soft">
        This browser view is only the internal Tauri frontend used while developing the desktop
        app. The real product is the native Windows app with tray, hotkey, mic HUD, transcription,
        and system-wide text insertion.
      </p>
      <div class="mt-8 flex flex-col items-center justify-center gap-3 sm:flex-row">
        <a
          href="https://github.com/guilhermeeng99/mia/releases"
          class="rounded-pill border-2 border-charcoal bg-charcoal px-7 py-3 text-body-lg font-bold text-surface"
        >
          Download for Windows
        </a>
        <a
          href="https://github.com/guilhermeeng99/mia"
          class="rounded-pill border-2 border-charcoal bg-surface px-7 py-3 text-body-lg font-bold text-charcoal"
        >
          View on GitHub
        </a>
      </div>
    </main>
  </div>
{:else if isHud}
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
