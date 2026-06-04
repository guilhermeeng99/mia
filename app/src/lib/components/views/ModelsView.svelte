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
  import { getSettings, updateSettings, type ModelSettings } from "../../settings";
  import ErrorBanner from "../ui/ErrorBanner.svelte";
  import Button from "../ui/Button.svelte";
  import Card from "../ui/Card.svelte";
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

  const MODEL_DETAILS: Record<
    string,
    { fidelity: string; latency: string; tone: "neutral" | "success" | "accent" | "info"; note: string }
  > = {
    small: {
      fidelity: "Fidelidade basica",
      latency: "Mais rapido",
      tone: "info",
      note: "Bom para respostas rapidas; pode errar mais em fala longa ou ambiente ruidoso.",
    },
    medium: {
      fidelity: "Fidelidade media",
      latency: "Equilibrado",
      tone: "neutral",
      note: "Mais fiel que Small, ainda viavel em CPU boa.",
    },
    "large-v3-turbo": {
      fidelity: "Fidelidade alta",
      latency: "Rapido na NVIDIA",
      tone: "success",
      note: "Melhor ponto de partida com GPU NVIDIA.",
    },
    "large-v3": {
      fidelity: "Fidelidade maxima",
      latency: "Mais lento",
      tone: "accent",
      note: "Mais pesado; escolha quando a qualidade vale esperar mais.",
    },
  };

  function detailsFor(id: string) {
    return MODEL_DETAILS[id] ?? MODEL_DETAILS.small;
  }

  function activeModelLabel() {
    return models.find((model) => model.id === activeModel)?.label ?? activeModel;
  }

  function warmLabel() {
    if (warm?.warming) return `aquecendo · ${warm.targetModel ?? activeModel}`;
    return warm?.loaded ? `quente · ${warm.model}` : "frio (nenhum modelo carregado)";
  }

  function fail(e: unknown) {
    error = String(e);
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
    warmStatus().then((w) => (warm = w)).catch(fail);
    gpuEngineStatus().then((g) => (gpu = g)).catch(fail);
  });

  async function selectModel(id: string) {
    error = null;
    try {
      const base = modelSettings ?? (await getSettings()).model;
      const s = await updateSettings({ model: { ...base, model: id } });
      modelSettings = s.model;
      activeModel = s.model.model;
      warm = await warmStatus();
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
      warm = await warmStatus();
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

  async function deleteModel(model: WhisperModel) {
    const confirmed = window.confirm(
      `Deletar o modelo ${model.label} baixado? Você poderá baixar novamente depois.`,
    );
    if (!confirmed) return;

    deletingModel = model.id;
    error = null;
    try {
      await deleteWhisperModel(model.id);
      await loadModels();
      warm = await warmStatus();
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
    } catch (e) {
      fail(e);
    } finally {
      gpuDownloading = false;
    }
  }
</script>

<PageHeader title="Modelos & Motor" subtitle="Baixe um modelo Whisper uma vez — depois, 100% offline." />

<ErrorBanner message={error} />

<div class="flex flex-col gap-6">
  <Card>
    <h2 class="font-display text-title">Modelo Whisper</h2>
    <p class="mt-1 text-body text-ink-soft">
      Baixado sob demanda do Hugging Face — a única saída de rede do MIA.
    </p>
    <p class="mt-2 text-body text-ink-soft">
      Modelo ativo: <span class="font-bold text-charcoal">{activeModelLabel()}</span>. Baixe modelos uma vez e
      selecione qual usar nas proximas falas.
    </p>
    <ul class="mt-4 flex flex-col gap-3">
      {#each models as model (model.id)}
        {@const details = detailsFor(model.id)}
        <li
          class="flex items-start gap-3 rounded-card border-2 border-charcoal px-4 py-3
                 {activeModel === model.id ? 'bg-surface' : 'bg-canvas'}"
        >
          <div class="flex min-w-0 flex-1 flex-col gap-2">
            <div class="flex flex-wrap items-center gap-2">
            <span class="text-body-lg font-bold">{model.label}</span>
            <span class="text-body text-ink-soft">{model.sizeMb} MB</span>
              <Pill tone={details.tone}>{details.fidelity}</Pill>
              <Pill tone="neutral">{details.latency}</Pill>
            {#if model.recommended}<Pill tone="accent">Padrao</Pill>{/if}
            </div>
            <p class="text-body text-ink-soft">{details.note}</p>
          </div>
          <div class="shrink-0 pt-1">
            {#if model.downloaded}
              <div class="flex flex-wrap items-center justify-end gap-2">
                {#if activeModel === model.id}
                  <Pill tone="success">✓ em uso</Pill>
                {:else}
                  <Button
                    variant="secondary"
                    size="sm"
                    disabled={downloading !== null || deletingModel !== null}
                    onclick={() => selectModel(model.id)}
                  >
                    Selecionar
                  </Button>
                {/if}
                <Button
                  variant="danger"
                  size="sm"
                  disabled={downloading !== null || deletingModel !== null}
                  onclick={() => deleteModel(model)}
                >
                  {deletingModel === model.id ? "Deletando" : "Deletar"}
                </Button>
              </div>
            {:else if downloading === model.id}
              <div class="flex items-center gap-2">
                <Pill tone="accent">baixando… {progress}%</Pill>
                <Button
                  variant="ghost"
                  size="sm"
                  disabled={cancellingDownload === model.id}
                  onclick={() => cancelDownload(model.id)}
                >
                  {cancellingDownload === model.id ? "Cancelando" : "Cancelar"}
                </Button>
              </div>
            {:else}
              <Button variant="secondary" size="sm" disabled={downloading !== null} onclick={() => download(model.id)}>
                Baixar
              </Button>
            {/if}
          </div>
        </li>
      {/each}
    </ul>
  </Card>

  <Card>
    <h2 class="font-display text-title">Aceleração</h2>
    <div class="mt-3 flex flex-wrap gap-3">
      <Pill tone={warm?.loaded ? "success" : warm?.warming ? "accent" : "neutral"}>
        {warmLabel()}
      </Pill>
      <Pill tone="neutral">backend: {warm?.backend ?? "—"}</Pill>
      {#if gpu?.gpuPresent}
        <Pill tone={gpu.downloaded ? "success" : "accent"}>
          GPU NVIDIA {gpu.downloaded ? "· engine pronto" : "· engine não baixado"}
        </Pill>
      {:else}
        <Pill tone="neutral">somente CPU</Pill>
      {/if}
    </div>
    {#if gpu?.gpuPresent && !gpu.downloaded}
      <div class="mt-4 flex flex-wrap items-center gap-3">
        {#if gpuDownloading}
          <Pill tone="accent">baixando engine… {gpuProgress}%</Pill>
        {:else}
          <Button variant="secondary" onclick={downloadGpu}>Baixar engine GPU (~435 MB)</Button>
        {/if}
        <span class="text-body text-ink-soft">~7–10× mais rápido; troca de motor na próxima fala.</span>
      </div>
    {/if}
  </Card>
</div>
