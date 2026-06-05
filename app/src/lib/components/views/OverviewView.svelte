<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { getHotkey, type HotkeyConfig } from "../../hotkey";
  import { i18n } from "../../i18n";
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
    if (warm?.warming) return $i18n.overview.warmWarming(warm.targetModel ?? "model");
    return warm?.loaded ? $i18n.overview.warmLoaded(warm.model ?? "Whisper") : $i18n.overview.warmCold;
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
    return new Intl.DateTimeFormat($i18n.dateLocale, {
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

<PageHeader title={$i18n.overview.title} subtitle={$i18n.overview.subtitle} />

<ErrorBanner message={error} />

<div class="flex flex-col gap-6">
  <Card tone="seafoam">
    <p class="text-body-lg font-bold">{$i18n.overview.readyTitle}</p>
    <p class="mt-2 max-w-[34ch] text-body-lg">
      {#if hotkey}
        {$i18n.overview.readyWithHotkey(hotkey.accelerator)}
      {:else}
        {$i18n.overview.readyWithoutHotkey}
      {/if}
    </p>
  </Card>

  <section>
    <div class="mb-3 flex items-center justify-between">
      <h2 class="font-display text-title">{$i18n.overview.usageTitle}</h2>
      {#if stats}
        <Button variant="danger" size="sm" onclick={() => (showResetConfirm = true)}>
          {$i18n.overview.resetStats}
        </Button>
      {/if}
    </div>
    {#if stats}
      <div class="grid grid-cols-2 gap-4 sm:grid-cols-4">
        <StatTile tone="sky" value={stats.totalWords} label={$i18n.overview.wordsDictated} />
        <StatTile tone="lavender" value={stats.avgWpm} label={$i18n.overview.avgWpm} />
        <StatTile tone="lemon" value={stats.dayStreak} label={$i18n.overview.dayStreak} />
        <StatTile tone="spring" value={stats.bestStreak} label={$i18n.overview.bestStreak} />
      </div>
    {:else}
      <p class="text-body text-ink-soft">{$i18n.generic.loading}</p>
    {/if}
  </section>

  <Card>
    <h2 class="font-display text-title">{$i18n.overview.engineTitle}</h2>
    <p class="mt-1 text-body text-ink-soft">{$i18n.overview.engineSubtitle}</p>
    <div class="mt-4 flex flex-wrap gap-3">
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
  </Card>

  <section>
    <div class="mb-3 flex items-center justify-between">
      <h2 class="font-display text-title">{$i18n.overview.historyTitle}</h2>
      {#if historyEntries.length > 0}
        <Button
          variant="danger"
          size="sm"
          disabled={historyLoading}
          onclick={() => (showClearConfirm = true)}
        >
          {$i18n.overview.clearAll}
        </Button>
      {/if}
    </div>
    <Card>
      {#if historyLoading}
        <p class="text-body text-ink-soft">{$i18n.generic.loading}</p>
      {:else if historyEntries.length === 0}
        <p class="text-body text-ink-soft">{$i18n.overview.noHistory}</p>
      {:else}
        <ul class="flex flex-col gap-3">
          {#each historyEntries as entry (entry.id)}
            <li class="group rounded-card border-2 border-charcoal bg-canvas px-4 py-3">
              <div class="flex flex-wrap items-center gap-2">
                <Pill tone="neutral">{formatDate(entry.createdAtMs)}</Pill>
                <Pill tone="info">{$i18n.overview.words(entry.wordCount)}</Pill>
                <div class="ml-auto flex gap-2 opacity-0 transition-opacity group-hover:opacity-100">
                  <Button variant="secondary" size="sm" onclick={() => copyEntry(entry.id)}>
                    {copiedId === entry.id ? $i18n.generic.copied : $i18n.generic.copy}
                  </Button>
                  <Button variant="ghost" size="sm" onclick={() => removeEntry(entry.id)}>
                    {$i18n.generic.remove}
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
  title={$i18n.overview.clearHistoryTitle}
  message={$i18n.overview.clearHistoryMessage}
  confirmLabel={$i18n.overview.clearAll}
  confirmVariant="danger"
  onconfirm={confirmClearAll}
  oncancel={() => (showClearConfirm = false)}
/>

<ConfirmDialog
  open={showResetConfirm}
  title={$i18n.overview.resetStatsTitle}
  message={$i18n.overview.resetStatsMessage}
  confirmLabel={$i18n.overview.reset}
  confirmVariant="danger"
  onconfirm={confirmResetStats}
  oncancel={() => (showResetConfirm = false)}
/>
