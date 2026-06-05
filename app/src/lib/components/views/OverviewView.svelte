<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { getHotkey, type HotkeyConfig } from "../../hotkey";
  import { getStats, resetStats, type UsageStats } from "../../stats";
  import { gpuEngineStatus, warmStatus, type GpuStatus, type WarmStatus } from "../../stt";
  import {
    listHistory,
    copyHistoryEntry,
    deleteHistoryEntry,
    clearHistory,
    type HistoryEntry,
  } from "../../history";
  import Button from "../ui/Button.svelte";
  import Card from "../ui/Card.svelte";
  import ConfirmDialog from "../ui/ConfirmDialog.svelte";
  import PageHeader from "../ui/PageHeader.svelte";
  import Pill from "../ui/Pill.svelte";
  import StatTile from "../ui/StatTile.svelte";
  import ErrorBanner from "../ui/ErrorBanner.svelte";

  // The landing view — warm greeting, live usage stats, engine status, and recent history.
  // Presentation only; reads through the typed wrappers.
  let stats = $state<UsageStats | null>(null);
  let warm = $state<WarmStatus | null>(null);
  let gpu = $state<GpuStatus | null>(null);
  let hotkey = $state<HotkeyConfig | null>(null);
  let error = $state<string | null>(null);
  let warmPoll: ReturnType<typeof setTimeout> | null = null;

  let historyEntries = $state<HistoryEntry[]>([]);
  let historyLoading = $state(true);
  let copiedId = $state<string | null>(null);
  let copyTimer: ReturnType<typeof setTimeout> | null = null;
  let showClearConfirm = $state(false);
  let showResetConfirm = $state(false);

  function fail(e: unknown) {
    error = String(e);
  }

  function warmLabel() {
    if (warm?.warming) return `aquecendo · ${warm.targetModel ?? "modelo"}`;
    return warm?.loaded ? `quente · ${warm.model}` : "frio (nenhum modelo carregado)";
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
  }

  async function reloadHistory() {
    historyLoading = true;
    try {
      historyEntries = await listHistory();
    } finally {
      historyLoading = false;
    }
  }

  function formatDate(ms: number) {
    return new Intl.DateTimeFormat("pt-BR", {
      day: "2-digit",
      month: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
    }).format(new Date(ms));
  }

  async function copyEntry(id: string) {
    error = null;
    try {
      await copyHistoryEntry(id);
      copiedId = id;
      if (copyTimer) clearTimeout(copyTimer);
      copyTimer = setTimeout(() => (copiedId = null), 1600);
    } catch (e) {
      fail(e);
    }
  }

  async function removeEntry(id: string) {
    error = null;
    try {
      await deleteHistoryEntry(id);
      await reloadHistory();
    } catch (e) {
      fail(e);
    }
  }

  async function confirmClearAll() {
    error = null;
    showClearConfirm = false;
    try {
      await clearHistory();
      historyEntries = [];
      copiedId = null;
    } catch (e) {
      fail(e);
    }
  }

  onMount(() => {
    getStats().then((s) => (stats = s)).catch(fail);
    refreshWarmStatus().catch(fail);
    gpuEngineStatus().then((g) => (gpu = g)).catch(fail);
    getHotkey().then((h) => (hotkey = h)).catch(fail);
    reloadHistory().catch(fail);

    let unlistenHistory: (() => void) | null = null;
    listen("history://saved", () => {
      reloadHistory().catch(fail);
    }).then((unlisten) => {
      unlistenHistory = unlisten;
    });

    return () => {
      clearWarmPoll();
      if (copyTimer) clearTimeout(copyTimer);
      unlistenHistory?.();
    };
  });

  // Local-only usage stats — never uploaded (ADR-001). Clear + refetch.
  async function confirmResetStats() {
    showResetConfirm = false;
    try {
      await resetStats();
      stats = await getStats();
    } catch (e) {
      fail(e);
    }
  }
</script>

<PageHeader title="Visão geral" subtitle="Sua voz, sua máquina. Ditado local para Windows." />

<ErrorBanner message={error} />

<div class="flex flex-col gap-6">
  <Card tone="seafoam">
    <p class="text-body-lg font-bold">Pronto para ditar</p>
    <p class="mt-2 max-w-[34ch] text-body-lg">
      {#if hotkey}
        Segure <span class="font-display">{hotkey.accelerator}</span> em qualquer app, fale, e o MIA
        digita o texto no cursor.
      {:else}
        Segure seu atalho em qualquer app, fale, e o MIA digita o texto no cursor.
      {/if}
    </p>
  </Card>

  <section>
    <div class="mb-3 flex items-center justify-between">
      <h2 class="font-display text-title">Seu uso</h2>
      {#if stats}
        <Button variant="danger" size="sm" onclick={() => (showResetConfirm = true)}>
          Zerar estatísticas
        </Button>
      {/if}
    </div>
    {#if stats}
      <div class="grid grid-cols-2 gap-4 sm:grid-cols-4">
        <StatTile tone="sky" value={stats.totalWords} label="palavras ditadas" />
        <StatTile tone="lavender" value={stats.avgWpm} label="WPM médio" />
        <StatTile tone="lemon" value={stats.dayStreak} label="dias seguidos" />
        <StatTile tone="spring" value={stats.bestStreak} label="melhor sequência" />
      </div>
    {:else}
      <p class="text-body text-ink-soft">Carregando…</p>
    {/if}
  </section>

  <Card>
    <h2 class="font-display text-title">Motor</h2>
    <p class="mt-1 text-body text-ink-soft">Locais — nada sai da máquina (ADR-001).</p>
    <div class="mt-4 flex flex-wrap gap-3">
      <Pill tone={warm?.warming ? "accent" : warm?.loaded ? "success" : "neutral"}>
        {warmLabel()}
      </Pill>
      <Pill tone={warm?.gpu ? "success" : "neutral"}>motor: {warm?.gpu ? "GPU" : "CPU"}</Pill>
      {#if gpu?.gpuPresent}
        <Pill tone={gpu.downloaded ? "success" : "accent"}>
          GPU NVIDIA {gpu.downloaded ? "· engine pronto" : "· engine não baixado"}
        </Pill>
      {:else}
        <Pill tone="neutral">somente CPU</Pill>
      {/if}
    </div>
  </Card>

  <section>
    <div class="mb-3 flex items-center justify-between">
      <h2 class="font-display text-title">Histórico</h2>
      {#if historyEntries.length > 0}
        <Button
          variant="danger"
          size="sm"
          disabled={historyLoading}
          onclick={() => (showClearConfirm = true)}
        >
          Limpar tudo
        </Button>
      {/if}
    </div>
    <Card>
      {#if historyLoading}
        <p class="text-body text-ink-soft">Carregando...</p>
      {:else if historyEntries.length === 0}
        <p class="text-body text-ink-soft">Nenhum texto ditado ainda.</p>
      {:else}
        <ul class="flex flex-col gap-3">
          {#each historyEntries as entry (entry.id)}
            <li class="group rounded-card border-2 border-charcoal bg-canvas px-4 py-3">
              <div class="flex flex-wrap items-center gap-2">
                <Pill tone="neutral">{formatDate(entry.createdAtMs)}</Pill>
                <Pill tone="info">{entry.wordCount} palavras</Pill>
                <div class="ml-auto flex gap-2 opacity-0 transition-opacity group-hover:opacity-100">
                  <Button variant="secondary" size="sm" onclick={() => copyEntry(entry.id)}>
                    {copiedId === entry.id ? "Copiado" : "Copiar"}
                  </Button>
                  <Button variant="ghost" size="sm" onclick={() => removeEntry(entry.id)}>
                    Remover
                  </Button>
                </div>
              </div>
              <p class="mt-3 whitespace-pre-wrap break-words text-body-lg text-charcoal">
                {entry.text}
              </p>
            </li>
          {/each}
        </ul>
      {/if}
    </Card>
  </section>
</div>

<ConfirmDialog
  open={showClearConfirm}
  title="Limpar histórico"
  message="Apagar todos os textos ditados? Esta ação não pode ser desfeita."
  confirmLabel="Limpar tudo"
  confirmVariant="danger"
  onconfirm={confirmClearAll}
  oncancel={() => (showClearConfirm = false)}
/>

<ConfirmDialog
  open={showResetConfirm}
  title="Zerar estatísticas"
  message="Apagar todas as estatísticas de uso? Esta ação não pode ser desfeita."
  confirmLabel="Zerar"
  confirmVariant="danger"
  onconfirm={confirmResetStats}
  oncancel={() => (showResetConfirm = false)}
/>
