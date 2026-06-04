<script lang="ts">
  import { onMount } from "svelte";
  import { getHotkey, type HotkeyConfig } from "../../hotkey";
  import { getStats, resetStats, type UsageStats } from "../../stats";
  import { gpuEngineStatus, warmStatus, type GpuStatus, type WarmStatus } from "../../stt";
  import Button from "../ui/Button.svelte";
  import Card from "../ui/Card.svelte";
  import PageHeader from "../ui/PageHeader.svelte";
  import Pill from "../ui/Pill.svelte";
  import StatTile from "../ui/StatTile.svelte";
  import ErrorBanner from "../ui/ErrorBanner.svelte";

  // The landing view — a warm greeting, the live usage stats, and the engine
  // status at a glance. Presentation only; reads through the typed wrappers.
  let stats = $state<UsageStats | null>(null);
  let warm = $state<WarmStatus | null>(null);
  let gpu = $state<GpuStatus | null>(null);
  let hotkey = $state<HotkeyConfig | null>(null);
  let error = $state<string | null>(null);

  function fail(e: unknown) {
    error = String(e);
  }

  function warmLabel() {
    if (warm?.warming) return `aquecendo · ${warm.targetModel ?? "modelo"}`;
    return warm?.loaded ? `quente · ${warm.model}` : "frio (nenhum modelo carregado)";
  }

  onMount(() => {
    getStats().then((s) => (stats = s)).catch(fail);
    warmStatus().then((w) => (warm = w)).catch(fail);
    gpuEngineStatus().then((g) => (gpu = g)).catch(fail);
    getHotkey().then((h) => (hotkey = h)).catch(fail);
  });

  // Local-only usage stats — never uploaded (ADR-001). Clear + refetch.
  async function resetUsageStats() {
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
    <h2 class="mb-3 font-display text-title">Seu uso</h2>
    {#if stats}
      <div class="grid grid-cols-2 gap-4 sm:grid-cols-4">
        <StatTile tone="sky" value={stats.totalWords} label="palavras ditadas" />
        <StatTile tone="lavender" value={stats.avgWpm} label="WPM médio" />
        <StatTile tone="lemon" value={stats.dayStreak} label="dias seguidos" />
        <StatTile tone="spring" value={stats.bestStreak} label="melhor sequência" />
      </div>
      <div class="mt-4">
        <Button variant="ghost" size="sm" onclick={resetUsageStats}>Zerar estatísticas</Button>
      </div>
    {:else}
      <p class="text-body text-ink-soft">Carregando…</p>
    {/if}
  </section>

  <Card>
    <h2 class="font-display text-title">Motor</h2>
    <p class="mt-1 text-body text-ink-soft">Locais — nada sai da máquina (ADR-001).</p>
    <div class="mt-4 flex flex-wrap gap-3">
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
  </Card>
</div>
