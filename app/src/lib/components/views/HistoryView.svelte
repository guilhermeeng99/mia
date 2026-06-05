<script lang="ts">
  import { onMount } from "svelte";
  import {
    clearHistory,
    copyHistoryEntry,
    deleteHistoryEntry,
    listHistory,
    type HistoryEntry,
  } from "../../history";
  import { i18n } from "../../i18n";
  import Button from "../ui/Button.svelte";
  import Card from "../ui/Card.svelte";
  import ErrorBanner from "../ui/ErrorBanner.svelte";
  import PageHeader from "../ui/PageHeader.svelte";
  import Pill from "../ui/Pill.svelte";

  let entries = $state<HistoryEntry[]>([]);
  let error = $state<string | null>(null);
  let loading = $state(true);
  let copiedId = $state<string | null>(null);
  let copyTimer: ReturnType<typeof setTimeout> | null = null;

  function fail(e: unknown) {
    error = String(e);
  }

  async function reload() {
    loading = true;
    try {
      entries = await listHistory();
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    reload().catch(fail);
  });

  function formatDate(ms: number) {
    return new Intl.DateTimeFormat($i18n.dateLocale, {
      day: "2-digit",
      month: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
    }).format(new Date(ms));
  }

  async function copy(id: string) {
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

  async function remove(id: string) {
    error = null;
    try {
      await deleteHistoryEntry(id);
      await reload();
    } catch (e) {
      fail(e);
    }
  }

  async function clearAll() {
    error = null;
    try {
      await clearHistory();
      entries = [];
      copiedId = null;
    } catch (e) {
      fail(e);
    }
  }
</script>

<PageHeader title={$i18n.history.title} subtitle={$i18n.history.subtitle}>
  {#snippet action()}
    <Button variant="danger" size="sm" disabled={loading || entries.length === 0} onclick={clearAll}>
      {$i18n.overview.clearAll}
    </Button>
  {/snippet}
</PageHeader>

<ErrorBanner message={error} />

<Card>
  {#if loading}
    <p class="text-body text-ink-soft">{$i18n.generic.loading}</p>
  {:else if entries.length === 0}
    <p class="text-body text-ink-soft">{$i18n.overview.noHistory}</p>
  {:else}
    <ul class="flex flex-col gap-3">
      {#each entries as entry (entry.id)}
        <li class="rounded-card border-2 border-charcoal bg-canvas px-4 py-3">
          <div class="flex flex-wrap items-center gap-2">
            <Pill tone="neutral">{formatDate(entry.createdAtMs)}</Pill>
            <Pill tone="info">{$i18n.overview.words(entry.wordCount)}</Pill>
            <div class="ml-auto flex gap-2">
              <Button variant="secondary" size="sm" onclick={() => copy(entry.id)}>
                {copiedId === entry.id ? $i18n.generic.copied : $i18n.generic.copy}
              </Button>
              <Button variant="ghost" size="sm" onclick={() => remove(entry.id)}>
                {$i18n.generic.remove}
              </Button>
            </div>
          </div>
          <p class="mt-3 whitespace-pre-wrap break-words text-body-lg text-charcoal">{entry.text}</p>
        </li>
      {/each}
    </ul>
  {/if}
</Card>
