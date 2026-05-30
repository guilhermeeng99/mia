<script lang="ts">
  import { Channel } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { isMicPermissionDenied, openMicPrivacy, testMicrophone } from "../audio";
  import { getHotkey } from "../hotkey";
  import { downloadWhisperModel, listWhisperModels, type DownloadProgress, type WhisperModel } from "../stt";
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
  let downloading = $state<string | null>(null);
  let progress = $state(0);
  let error = $state<string | null>(null);

  const steps = ["Bem-vindo", "Atalho", "Microfone", "Modelo"];

  async function refreshModels() {
    models = await listWhisperModels();
  }

  onMount(() => {
    getHotkey().then((h) => (chord = h.accelerator)).catch(() => {});
    refreshModels().catch((e) => (error = String(e)));
  });

  async function runMicTest() {
    micTesting = true;
    micMsg = null;
    micDenied = false;
    micLevel = 0;
    try {
      const r = await testMicrophone(1500, (rms) => (micLevel = rms));
      micMsg = r.peak > 0.02 ? "Ouvimos você! 🎤" : "Quase nenhum som — confira o microfone.";
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
      await refreshModels();
    } catch (e) {
      error = String(e);
    } finally {
      downloading = null;
    }
  }

  const hasModel = $derived(models.some((m) => m.downloaded));
</script>

<main class="grid min-h-screen place-items-center bg-canvas px-6 py-8 font-body text-charcoal">
  <div class="w-full max-w-[560px]">
    <div class="mb-5 flex items-center gap-2">
      <span class="font-display text-[1.6rem] leading-none">MIA</span>
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
        <h1 class="font-display text-hero leading-none">Bem-vindo ao MIA</h1>
        <p class="mt-4 text-body-lg text-ink-soft">
          Ditado por voz <strong class="text-charcoal">100% local</strong> para Windows. Sua voz
          nunca sai da máquina.
        </p>
        <div class="mt-6"><Button onclick={() => (step = 1)}>Começar</Button></div>
      {:else if step === 1}
        <h1 class="font-display text-page">Seu atalho</h1>
        <p class="mt-3 text-body-lg text-ink-soft">
          Segure <span class="font-display text-charcoal">{chord}</span> e fale; solte para inserir
          o texto onde o cursor estiver.
        </p>
        <div class="mt-6 flex gap-3">
          <Button variant="secondary" onclick={() => (step = 0)}>Voltar</Button>
          <Button onclick={() => (step = 2)}>Próximo</Button>
        </div>
      {:else if step === 2}
        <h1 class="font-display text-page">Testar microfone</h1>
        <p class="mt-3 text-body-lg text-ink-soft">Fale algo e confirme que estamos ouvindo.</p>
        <div class="mt-4 flex items-center gap-3">
          <Button variant="secondary" disabled={micTesting} onclick={runMicTest}>
            {micTesting ? "Ouvindo…" : "Testar"}
          </Button>
          {#if micTesting}
            <LevelMeter level={micLevel} />
          {:else if micMsg}
            <span class="text-body text-ink-soft">{micMsg}</span>
          {/if}
        </div>
        {#if micDenied}
          <div class="mt-3 flex flex-wrap items-center gap-3">
            <span class="text-body text-danger">Acesso ao microfone bloqueado pelo Windows.</span>
            <Button variant="secondary" size="sm" onclick={openMicSettings}>Abrir configurações</Button>
          </div>
        {/if}
        <div class="mt-6 flex gap-3">
          <Button variant="secondary" onclick={() => (step = 1)}>Voltar</Button>
          <Button onclick={() => (step = 3)}>Próximo</Button>
        </div>
      {:else}
        <h1 class="font-display text-page">Baixar o modelo</h1>
        <p class="mt-3 text-body-lg text-ink-soft">
          Baixe um modelo (uma única vez). <strong class="text-charcoal">Small</strong> é o recomendado.
        </p>
        <ul class="mt-4 flex flex-col gap-3">
          {#each models as model (model.id)}
            <li class="flex items-center gap-3 rounded-card border-2 border-charcoal bg-canvas px-4 py-3">
              <div class="min-w-0 flex-1">
                <div class="flex flex-wrap items-center gap-2">
                  <span class="text-body-lg font-bold">{model.label}</span>
                  {#if model.recommended}<Pill tone="accent">Recomendado</Pill>{/if}
                </div>
                <span class="text-body text-ink-soft">{model.sizeMb} MB</span>
              </div>
              <div class="shrink-0">
                {#if model.downloaded}
                  <Pill tone="success">✓ instalado</Pill>
                {:else if downloading === model.id}
                  <Pill tone="accent">{progress}%</Pill>
                {:else}
                  <Button variant="secondary" size="sm" disabled={downloading !== null} onclick={() => download(model.id)}>
                    Baixar
                  </Button>
                {/if}
              </div>
            </li>
          {/each}
        </ul>
        <div class="mt-6 flex gap-3">
          <Button variant="secondary" disabled={downloading !== null} onclick={() => (step = 2)}>Voltar</Button>
          <Button disabled={!hasModel || downloading !== null} onclick={ondone}>Concluir</Button>
        </div>
      {/if}
    </Card>
  </div>
</main>
