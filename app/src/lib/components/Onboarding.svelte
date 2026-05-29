<script lang="ts">
  import { Channel } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { testMicrophone } from "../audio";
  import { getHotkey } from "../hotkey";
  import { downloadWhisperModel, listWhisperModels, type DownloadProgress, type WhisperModel } from "../stt";
  import Button from "./ui/Button.svelte";
  import Card from "./ui/Card.svelte";

  // First-run wizard (Phase 4) — welcome → hotkey → mic test → model download.
  // Presentation only; reuses the typed wrappers. `ondone` returns to the Hub.
  let { ondone }: { ondone: () => void } = $props();

  let step = $state(0);
  let chord = $state("Ctrl+Space");
  let micMsg = $state<string | null>(null);
  let micTesting = $state(false);
  let models = $state<WhisperModel[]>([]);
  let downloading = $state(false);
  let progress = $state(0);
  let error = $state<string | null>(null);

  const steps = ["Bem-vindo", "Atalho", "Microfone", "Modelo"];

  onMount(() => {
    getHotkey().then((h) => (chord = h.accelerator)).catch(() => {});
    listWhisperModels().then((m) => (models = m)).catch((e) => (error = String(e)));
  });

  async function runMicTest() {
    micTesting = true;
    micMsg = null;
    try {
      const r = await testMicrophone(1500);
      micMsg = r.peak > 0.02 ? "Ouvimos você! 🎤" : "Quase nenhum som — confira o microfone.";
    } catch (e) {
      error = String(e);
    } finally {
      micTesting = false;
    }
  }

  async function downloadRecommended() {
    const target = models.find((m) => m.id === "small") ?? models[0];
    if (!target) return;
    downloading = true;
    progress = 0;
    error = null;
    try {
      const ch = new Channel<DownloadProgress>();
      ch.onmessage = (p) => (progress = Math.round(p.percent));
      await downloadWhisperModel(target.id, ch);
      ondone();
    } catch (e) {
      error = String(e);
    } finally {
      downloading = false;
    }
  }

  const hasModel = $derived(models.some((m) => m.downloaded));
</script>

<main class="min-h-screen bg-cloud-mist text-midnight-indigo font-gilroy grid place-items-center px-6 py-8">
  <Card class="w-full max-w-[560px]">
    <div class="flex items-center gap-2 text-body text-slate-blue">
      {#each steps as label, i (label)}
        <span class={i === step ? "text-action-blue font-semibold" : ""}>{label}</span>
        {#if i < steps.length - 1}<span>·</span>{/if}
      {/each}
    </div>

    {#if error}
      <p class="mt-3 text-body text-danger">⚠ {error}</p>
    {/if}

    {#if step === 0}
      <h1 class="mt-4 text-heading-lg font-bold">Bem-vindo ao MIA</h1>
      <p class="mt-2 text-body-lg text-slate-blue">
        Ditado por voz <strong>100% local</strong> para Windows. Sua voz nunca sai da máquina.
      </p>
      <div class="mt-6"><Button onclick={() => (step = 1)}>Começar</Button></div>
    {:else if step === 1}
      <h1 class="mt-4 text-heading font-semibold">Seu atalho</h1>
      <p class="mt-2 text-body-lg text-slate-blue">
        Segure <strong>{chord}</strong> e fale; solte para inserir o texto onde o cursor estiver.
      </p>
      <div class="mt-6 flex gap-3">
        <Button variant="secondary" onclick={() => (step = 0)}>Voltar</Button>
        <Button onclick={() => (step = 2)}>Próximo</Button>
      </div>
    {:else if step === 2}
      <h1 class="mt-4 text-heading font-semibold">Testar microfone</h1>
      <p class="mt-2 text-body-lg text-slate-blue">Fale algo e confirme que estamos ouvindo.</p>
      <div class="mt-4 flex items-center gap-3">
        <Button variant="secondary" disabled={micTesting} onclick={runMicTest}>
          {micTesting ? "Ouvindo…" : "Testar"}
        </Button>
        {#if micMsg}<span class="text-body text-slate-blue">{micMsg}</span>{/if}
      </div>
      <div class="mt-6 flex gap-3">
        <Button variant="secondary" onclick={() => (step = 1)}>Voltar</Button>
        <Button onclick={() => (step = 3)}>Próximo</Button>
      </div>
    {:else}
      <h1 class="mt-4 text-heading font-semibold">Baixar o modelo</h1>
      <p class="mt-2 text-body-lg text-slate-blue">
        Um modelo Whisper é baixado uma vez do Hugging Face (a única saída de rede).
      </p>
      {#if downloading}
        <p class="mt-4 text-body-lg text-action-blue">Baixando… {progress}%</p>
      {/if}
      <div class="mt-6 flex gap-3">
        <Button variant="secondary" onclick={() => (step = 2)}>Voltar</Button>
        {#if hasModel}
          <Button onclick={ondone}>Concluir</Button>
        {:else}
          <Button disabled={downloading} onclick={downloadRecommended}>Baixar e concluir</Button>
        {/if}
        <Button variant="ghost" onclick={ondone}>Pular</Button>
      </div>
    {/if}
  </Card>
</main>
