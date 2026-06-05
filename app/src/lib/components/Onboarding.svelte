<script lang="ts">
  import { Channel } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { isMicPermissionDenied, openMicPrivacy, testMicrophone } from "../audio";
  import { getHotkey } from "../hotkey";
  import { i18n } from "../i18n";
  import { getSettings, updateSettings, type ModelSettings } from "../settings";
  import {
    cancelWhisperModelDownload,
    downloadWhisperModel,
    listWhisperModels,
    type DownloadProgress,
    type WhisperModel,
  } from "../stt";
  import Button from "./ui/Button.svelte";
  import Card from "./ui/Card.svelte";
  import LevelMeter from "./ui/LevelMeter.svelte";
  import Pill from "./ui/Pill.svelte";

  // First-run wizard (Phase 4) — welcome → hotkey → mic test → model download.
  // Presentation only; reuses the typed wrappers. `ondone` returns to the Hub.
  let { ondone }: { ondone: () => void } = $props();

  // The Model step mirrors the Hub: all registry models are offered with sizes so
  // the user can pick (onboarding.md Rule 7), `small` is flagged `recommended` by the
  // engine, and the step is mandatory — there is no skip and "Concluir" stays disabled
  // until a model is on disk (Rule 6: dictation is impossible without one).
  let step = $state(0);
  let chord = $state("Ctrl+Space");
  let micMsg = $state<string | null>(null);
  let micTesting = $state(false);
  let micLevel = $state(0);
  let micDenied = $state(false);
  let models = $state<WhisperModel[]>([]);
  let modelSettings = $state<ModelSettings | null>(null);
  let selectedModel = $state("small");
  let downloading = $state<string | null>(null);
  let cancellingDownload = $state<string | null>(null);
  let progress = $state(0);
  let error = $state<string | null>(null);

  const steps = $derived($i18n.onboarding.steps);

  async function refreshModels() {
    models = await listWhisperModels();
  }

  onMount(() => {
    getHotkey().then((h) => (chord = h.accelerator)).catch(() => {});
    getSettings()
      .then((s) => {
        modelSettings = s.model;
        selectedModel = s.model.model;
      })
      .catch((e) => (error = String(e)));
    refreshModels().catch((e) => (error = String(e)));
  });

  async function runMicTest() {
    micTesting = true;
    micMsg = null;
    micDenied = false;
    micLevel = 0;
    try {
      const r = await testMicrophone(1500, (rms) => (micLevel = rms));
      micMsg = r.peak > 0.02 ? $i18n.onboarding.micHeard : $i18n.onboarding.micQuiet;
    } catch (e) {
      micDenied = isMicPermissionDenied(String(e));
      error = String(e);
    } finally {
      micTesting = false;
      micLevel = 0;
    }
  }

  function openMicSettings() {
    openMicPrivacy().catch((e) => (error = String(e)));
  }

  async function download(id: string) {
    downloading = id;
    progress = 0;
    error = null;
    try {
      const ch = new Channel<DownloadProgress>();
      ch.onmessage = (p) => (progress = Math.round(p.percent));
      await downloadWhisperModel(id, ch);
      await selectModel(id);
      await refreshModels();
    } catch (e) {
      if (!String(e).toLowerCase().includes("cancelled")) error = String(e);
    } finally {
      downloading = null;
      cancellingDownload = null;
      await refreshModels();
    }
  }

  async function cancelDownload(id: string) {
    cancellingDownload = id;
    try {
      await cancelWhisperModelDownload(id);
    } catch (e) {
      error = String(e);
      cancellingDownload = null;
    }
  }

  async function selectModel(id: string) {
    const base = modelSettings ?? (await getSettings()).model;
    const s = await updateSettings({ model: { ...base, model: id } });
    modelSettings = s.model;
    selectedModel = s.model.model;
  }

  const hasSelectedModel = $derived(models.some((m) => m.id === selectedModel && m.downloaded));
</script>

<main class="grid min-h-full place-items-center bg-canvas px-6 py-8 font-body text-charcoal">
  <div class="w-full max-w-[560px]">
    <div class="mb-5 flex items-center gap-3">
      <img src="/logo.png" alt="MIA" class="h-11 w-auto" />
      <span class="ml-auto flex items-center gap-1.5 text-caption font-bold text-ink-soft">
        {#each steps as label, i (label)}
          <span class={i === step ? "text-charcoal" : ""}>{label}</span>
          {#if i < steps.length - 1}<span class="text-hairline">·</span>{/if}
        {/each}
      </span>
    </div>

    <Card>
      {#if error}
        <p class="mb-3 text-body text-danger">⚠ {error}</p>
      {/if}

      {#if step === 0}
        <h1 class="font-display text-hero leading-none">{$i18n.onboarding.welcomeTitle}</h1>
        <p class="mt-4 text-body-lg text-ink-soft">
          {$i18n.onboarding.welcomeBody}
        </p>
        <div class="mt-6"><Button onclick={() => (step = 1)}>{$i18n.onboarding.start}</Button></div>
      {:else if step === 1}
        <h1 class="font-display text-page">{$i18n.onboarding.shortcutTitle}</h1>
        <p class="mt-3 text-body-lg text-ink-soft">
          {$i18n.onboarding.shortcutBody(chord)}
        </p>
        <div class="mt-6 flex gap-3">
          <Button variant="secondary" onclick={() => (step = 0)}>{$i18n.generic.back}</Button>
          <Button onclick={() => (step = 2)}>{$i18n.generic.next}</Button>
        </div>
      {:else if step === 2}
        <h1 class="font-display text-page">{$i18n.onboarding.micTitle}</h1>
        <p class="mt-3 text-body-lg text-ink-soft">{$i18n.onboarding.micBody}</p>
        <div class="mt-4 flex items-center gap-3">
          <Button variant="secondary" disabled={micTesting} onclick={runMicTest}>
            {micTesting ? $i18n.onboarding.listening : $i18n.onboarding.test}
          </Button>
          {#if micTesting}
            <LevelMeter level={micLevel} />
          {:else if micMsg}
            <span class="text-body text-ink-soft">{micMsg}</span>
          {/if}
        </div>
        {#if micDenied}
          <div class="mt-3 flex flex-wrap items-center gap-3">
            <span class="text-body text-danger">{$i18n.onboarding.micBlocked}</span>
            <Button variant="secondary" size="sm" onclick={openMicSettings}>{$i18n.onboarding.openSettings}</Button>
          </div>
        {/if}
        <div class="mt-6 flex gap-3">
          <Button variant="secondary" onclick={() => (step = 1)}>{$i18n.generic.back}</Button>
          <Button onclick={() => (step = 3)}>{$i18n.generic.next}</Button>
        </div>
      {:else}
        <h1 class="font-display text-page">{$i18n.onboarding.modelTitle}</h1>
        <p class="mt-3 text-body-lg text-ink-soft">
          {$i18n.onboarding.modelBody}
        </p>
        <ul class="mt-4 flex flex-col gap-3">
          {#each models as model (model.id)}
            <li class="flex items-center gap-3 rounded-card border-2 border-charcoal bg-canvas px-4 py-3">
              <div class="min-w-0 flex-1">
                <div class="flex flex-wrap items-center gap-2">
                  <span class="text-body-lg font-bold">{model.label}</span>
                  {#if model.recommended}<Pill tone="accent">{$i18n.generic.recommended}</Pill>{/if}
                </div>
                <span class="text-body text-ink-soft">{model.sizeMb} MB</span>
              </div>
              <div class="shrink-0">
                {#if model.downloaded}
                  {#if selectedModel === model.id}
                    <Pill tone="success">✓ {$i18n.onboarding.selectedInUse}</Pill>
                  {:else}
                    <Button
                      variant="secondary"
                      size="sm"
                      disabled={downloading !== null}
                      onclick={() => selectModel(model.id).catch((e) => (error = String(e)))}
                    >
                      {$i18n.generic.use}
                    </Button>
                  {/if}
                {:else if downloading === model.id}
                  <div class="flex items-center gap-2">
                    <Pill tone="accent">{progress}%</Pill>
                    <Button
                      variant="ghost"
                      size="sm"
                      disabled={cancellingDownload === model.id}
                      onclick={() => cancelDownload(model.id)}
                    >
                      {cancellingDownload === model.id ? $i18n.generic.canceling : $i18n.generic.cancel}
                    </Button>
                  </div>
                {:else}
                  <Button variant="secondary" size="sm" disabled={downloading !== null} onclick={() => download(model.id)}>
                    {$i18n.generic.download}
                  </Button>
                {/if}
              </div>
            </li>
          {/each}
        </ul>
        <div class="mt-6 flex gap-3">
          <Button variant="secondary" disabled={downloading !== null} onclick={() => (step = 2)}>{$i18n.generic.back}</Button>
          <Button disabled={!hasSelectedModel || downloading !== null} onclick={ondone}>{$i18n.generic.finish}</Button>
        </div>
      {/if}
    </Card>
  </div>
</main>
