<script lang="ts">
  import { Channel } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import {
    cancelWhisperModelDownload,
    deleteWhisperModel,
    downloadGpuEngine,
    downloadWhisperModel,
    gpuEngineStatus,
    listWhisperModels,
    warmStatus,
    type DownloadProgress,
    type GpuStatus,
    type WarmStatus,
    type WhisperModel,
  } from "../../stt";
  import { i18n } from "../../i18n";
  import { getSettings, updateSettings, type ModelSettings } from "../../settings";
  import ErrorBanner from "../ui/ErrorBanner.svelte";
  import Button from "../ui/Button.svelte";
  import Card from "../ui/Card.svelte";
  import ConfirmDialog from "../ui/ConfirmDialog.svelte";
  import PageHeader from "../ui/PageHeader.svelte";
  import Pill from "../ui/Pill.svelte";

  // The model + engine view — download Whisper models on demand (the one network
  // call MIA makes, ADR-007) and the optional NVIDIA CUDA engine. Presentation only.
  let models = $state<WhisperModel[]>([]);
  let modelSettings = $state<ModelSettings | null>(null);
  let activeModel = $state("small");
  let warm = $state<WarmStatus | null>(null);
  let gpu = $state<GpuStatus | null>(null);
  let downloading = $state<string | null>(null);
  let cancellingDownload = $state<string | null>(null);
  let deletingModel = $state<string | null>(null);
  let progress = $state(0);
  let gpuDownloading = $state(false);
  let gpuProgress = $state(0);
  let error = $state<string | null>(null);
  let warmPoll: ReturnType<typeof setTimeout> | null = null;
  let confirmDeleteModel = $state<WhisperModel | null>(null);

  function detailsFor(id: string) {
    const tones: Record<string, "neutral" | "success" | "accent" | "info"> = {
      small: "info",
      medium: "neutral",
      "large-v3-turbo": "success",
      "large-v3": "accent",
    };
    const details =
      id === "medium"
        ? $i18n.models.details.medium
        : id === "large-v3-turbo"
          ? $i18n.models.details.turbo
          : id === "large-v3"
            ? $i18n.models.details.large
            : $i18n.models.details.small;
    return { ...details, tone: tones[id] ?? "info" };
  }

  function activeModelLabel() {
    return models.find((model) => model.id === activeModel)?.label ?? activeModel;
  }

  function warmLabel() {
    if (warm?.warming) return $i18n.overview.warmWarming(warm.targetModel ?? activeModel);
    return warm?.loaded ? $i18n.overview.warmLoaded(warm.model ?? "Whisper") : $i18n.overview.warmCold;
  }

  function fail(e: unknown) {
    error = String(e);
  }

  function clearWarmPoll() {
    if (warmPoll !== null) {
      clearTimeout(warmPoll);
      warmPoll = null;
    }
  }

  async function refreshWarmStatus() {
    clearWarmPoll();
    const next = await warmStatus();
    warm = next;
    if (next.warming) {
      warmPoll = setTimeout(() => {
        warmPoll = null;
        refreshWarmStatus().catch(fail);
      }, 1000);
    }
    return next;
  }

  async function loadModels() {
    models = await listWhisperModels();
  }

  onMount(() => {
    loadModels().catch(fail);
    getSettings()
      .then((s) => {
        modelSettings = s.model;
        activeModel = s.model.model;
      })
      .catch(fail);
    refreshWarmStatus().catch(fail);
    gpuEngineStatus().then((g) => (gpu = g)).catch(fail);
    return clearWarmPoll;
  });

  async function selectModel(id: string) {
    error = null;
    try {
      const base = modelSettings ?? (await getSettings()).model;
      const s = await updateSettings({ model: { ...base, model: id } });
      modelSettings = s.model;
      activeModel = s.model.model;
      await refreshWarmStatus();
    } catch (e) {
      fail(e);
    }
  }

  async function download(id: string) {
    downloading = id;
    progress = 0;
    error = null;
    try {
      const channel = new Channel<DownloadProgress>();
      channel.onmessage = (p) => (progress = Math.round(p.percent));
      await downloadWhisperModel(id, channel);
      await loadModels();
      await refreshWarmStatus();
    } catch (e) {
      if (!String(e).toLowerCase().includes("cancelled")) fail(e);
    } finally {
      downloading = null;
      cancellingDownload = null;
      await loadModels();
    }
  }

  async function cancelDownload(id: string) {
    cancellingDownload = id;
    try {
      await cancelWhisperModelDownload(id);
    } catch (e) {
      fail(e);
      cancellingDownload = null;
    }
  }

  async function confirmDelete() {
    const model = confirmDeleteModel;
    confirmDeleteModel = null;
    if (!model) return;

    deletingModel = model.id;
    error = null;
    try {
      await deleteWhisperModel(model.id);
      await loadModels();
      await refreshWarmStatus();
    } catch (e) {
      fail(e);
    } finally {
      deletingModel = null;
    }
  }

  // Download the optional NVIDIA CUDA whisper engine (~435 MB) into app-data; once
  // present, the warm engine spawns the GPU build instead of CPU (~7-10x faster).
  async function downloadGpu() {
    gpuDownloading = true;
    gpuProgress = 0;
    error = null;
    try {
      const channel = new Channel<DownloadProgress>();
      channel.onmessage = (p) => (gpuProgress = Math.round(p.percent));
      await downloadGpuEngine(channel);
      gpu = await gpuEngineStatus();
      await refreshWarmStatus();
    } catch (e) {
      fail(e);
    } finally {
      gpuDownloading = false;
    }
  }
</script>

<PageHeader title={$i18n.models.title} subtitle={$i18n.models.subtitle} />

<ErrorBanner message={error} />

<div class="flex flex-col gap-6">
  <Card>
    <h2 class="font-display text-title">{$i18n.models.whisperTitle}</h2>
    <p class="mt-1 text-body text-ink-soft">
      {$i18n.models.whisperSubtitle}
    </p>
    <p class="mt-2 text-body text-ink-soft">
      {$i18n.models.activeModel(activeModelLabel())}
    </p>
    <ul class="mt-4 flex flex-col gap-3">
      {#each models as model (model.id)}
        {@const details = detailsFor(model.id)}
        <li
          class="group flex items-start gap-3 rounded-card border-2 px-4 py-3
                 {activeModel === model.id
                   ? 'border-spring-mid bg-spring-light'
                   : 'border-charcoal bg-canvas'}"
        >
          <div class="flex min-w-0 flex-1 flex-col gap-2">
            <div class="flex flex-wrap items-center gap-2">
              <span class="text-body-lg font-bold {activeModel === model.id ? 'text-success' : ''}">
                {activeModel === model.id ? $i18n.models.selectedPrefix : ''}{model.label}
              </span>
              <span class="text-body text-ink-soft">{model.sizeMb} MB</span>
              <Pill tone={details.tone}>{details.fidelity}</Pill>
              <Pill tone="neutral">{details.latency}</Pill>
              {#if model.recommended}<Pill tone="accent">{$i18n.models.defaultPill}</Pill>{/if}
            </div>
            <p class="text-body text-ink-soft">{details.note}</p>
          </div>
          <div class="shrink-0 pt-1">
            {#if model.downloaded}
              <div class="flex flex-wrap items-center justify-end gap-2">
                <div class="opacity-0 transition-opacity group-hover:opacity-100">
                  <Button
                    variant="danger"
                    size="sm"
                    disabled={downloading !== null || deletingModel !== null}
                    onclick={() => (confirmDeleteModel = model)}
                  >
                    {deletingModel === model.id ? $i18n.generic.deleting : $i18n.generic.delete}
                  </Button>
                </div>
                {#if activeModel === model.id}
                  <Pill tone="success">{$i18n.generic.selected}</Pill>
                {:else}
                  <div class="opacity-0 transition-opacity group-hover:opacity-100">
                    <Button
                      variant="secondary"
                      size="sm"
                      disabled={downloading !== null || deletingModel !== null}
                      onclick={() => selectModel(model.id)}
                    >
                      {$i18n.generic.select}
                    </Button>
                  </div>
                {/if}
              </div>
            {:else if downloading === model.id}
              <div class="flex items-center gap-2">
                <Pill tone="accent">{$i18n.models.downloadingModel(progress)}</Pill>
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
              <div class="opacity-0 transition-opacity group-hover:opacity-100">
                <Button
                  variant="secondary"
                  size="sm"
                  disabled={downloading !== null}
                  onclick={() => download(model.id)}
                >
                  {$i18n.generic.download}
                </Button>
              </div>
            {/if}
          </div>
        </li>
      {/each}
    </ul>
  </Card>

  <Card>
    <h2 class="font-display text-title">{$i18n.models.accelerationTitle}</h2>
    <div class="mt-3 flex flex-wrap gap-3">
      <Pill tone={warm?.warming ? "accent" : warm?.loaded ? "success" : "neutral"}>
        {warmLabel()}
      </Pill>
      <Pill tone={warm?.gpu ? "success" : "neutral"}>{$i18n.overview.engineMode(warm?.gpu ? "GPU" : "CPU")}</Pill>
      {#if gpu?.gpuPresent}
        <Pill tone={gpu.downloaded ? "success" : "accent"}>
          {gpu.downloaded ? $i18n.overview.gpuReady : $i18n.overview.gpuMissingEngine}
        </Pill>
      {:else}
        <Pill tone="neutral">{$i18n.overview.cpuOnly}</Pill>
      {/if}
    </div>
    {#if gpu?.gpuPresent && !gpu.downloaded}
      <div class="mt-4 flex flex-wrap items-center gap-3">
        {#if gpuDownloading}
          <Pill tone="accent">{$i18n.models.downloadingEngine(gpuProgress)}</Pill>
        {:else}
          <Button variant="secondary" onclick={downloadGpu}>{$i18n.models.downloadGpu}</Button>
        {/if}
        <span class="text-body text-ink-soft">{$i18n.models.gpuHint}</span>
      </div>
    {/if}
  </Card>
</div>

<ConfirmDialog
  open={confirmDeleteModel !== null}
  title={$i18n.models.deleteTitle}
  message={$i18n.models.deleteMessage(confirmDeleteModel?.label ?? "")}
  confirmLabel={$i18n.generic.delete}
  confirmVariant="danger"
  onconfirm={confirmDelete}
  oncancel={() => (confirmDeleteModel = null)}
/>
