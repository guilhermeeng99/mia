<script lang="ts">
  import { Channel } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { listInputDevices, testMicrophone, type AudioDevice } from "../audio";
  import { injectText } from "../inject";
  import { checkForUpdate, installUpdate, type Update } from "../update";
  import { getHotkey, updateHotkey, type ActivationMode, type HotkeyConfig } from "../hotkey";
  import { getSettings, updateSettings, type GeneralSettings } from "../settings";
  import {
    downloadGpuEngine,
    downloadWhisperModel,
    gpuEngineStatus,
    listWhisperModels,
    warmStatus,
    type DownloadProgress,
    type GpuStatus,
    type WarmStatus,
    type WhisperModel,
  } from "../stt";
  import Button from "./ui/Button.svelte";
  import Card from "./ui/Card.svelte";
  import Field from "./ui/Field.svelte";
  import Pill from "./ui/Pill.svelte";
  import Toggle from "./ui/Toggle.svelte";
  import DictionarySection from "./DictionarySection.svelte";
  import SnippetsSection from "./SnippetsSection.svelte";

  // The Settings/Hub window — presentation only. All logic lives behind the typed
  // invoke() wrappers in lib/*.ts (architecture rule); this component never calls
  // invoke directly. Commands the engine doesn't expose yet (device persistence,
  // hotkey) are intentionally absent until their runtime stage lands.

  let { version }: { version: string } = $props();

  let devices = $state<AudioDevice[]>([]);
  let selectedDevice = $state("");
  let models = $state<WhisperModel[]>([]);
  let warm = $state<WarmStatus | null>(null);
  let gpu = $state<GpuStatus | null>(null);
  let downloading = $state<string | null>(null);
  let progress = $state(0);
  let gpuDownloading = $state(false);
  let gpuProgress = $state(0);
  let testText = $state("Olá do MIA — teste de injeção. 🎤");
  let injectMsg = $state<string | null>(null);
  let micMsg = $state<string | null>(null);
  let micTesting = $state(false);
  let general = $state<GeneralSettings | null>(null);
  let hotkey = $state<HotkeyConfig | null>(null);
  let recording = $state(false);
  let hotkeyError = $state<string | null>(null);
  let update = $state<Update | null>(null);
  let updateBusy = $state(false);
  let error = $state<string | null>(null);

  function fail(e: unknown) {
    error = String(e);
  }

  // Download + install the available update, then relaunch (the plugin handles it).
  async function applyUpdate() {
    if (!update) return;
    updateBusy = true;
    try {
      await installUpdate(update); // downloads, installs, relaunches
    } catch (e) {
      fail(e);
      updateBusy = false;
    }
  }

  // Map a KeyboardEvent.code to MIA's canonical key token (matches the Rust parser).
  function keyFromCode(code: string): string | null {
    if (/^Key[A-Z]$/.test(code)) return code.slice(3);
    if (/^Digit[0-9]$/.test(code)) return code.slice(5);
    if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
    const named: Record<string, string> = {
      Space: "Space", Tab: "Tab", Enter: "Enter", Escape: "Escape", Delete: "Delete",
      ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right",
    };
    return named[code] ?? null;
  }

  // Build a canonical accelerator (e.g. "Ctrl+Shift+D") or null while still waiting
  // for a modifier+key chord (a bare key is rejected by the engine, Rule 5).
  function accelFromEvent(e: KeyboardEvent): string | null {
    const mods: string[] = [];
    if (e.ctrlKey) mods.push("Ctrl");
    if (e.altKey) mods.push("Alt");
    if (e.shiftKey) mods.push("Shift");
    if (e.metaKey) mods.push("Super");
    const key = keyFromCode(e.code);
    if (!key || mods.length === 0) return null;
    return [...mods, key].join("+");
  }

  function onRecordKey(e: KeyboardEvent) {
    if (!recording) return;
    e.preventDefault();
    if (e.code === "Escape" && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey) {
      stopRecording();
      return;
    }
    const accel = accelFromEvent(e);
    if (accel) void commitHotkey(accel);
  }

  function startRecording() {
    hotkeyError = null;
    recording = true;
    window.addEventListener("keydown", onRecordKey, true);
  }

  function stopRecording() {
    recording = false;
    window.removeEventListener("keydown", onRecordKey, true);
  }

  // Persist + re-register via update_hotkey, which rejects a conflicting chord
  // (the engine's conflict-probe) and keeps the old binding on failure.
  async function commitHotkey(accelerator: string) {
    stopRecording();
    const mode = hotkey?.mode ?? "pushToHold";
    try {
      await updateHotkey({ accelerator, mode });
      hotkey = { accelerator, mode };
    } catch (e) {
      hotkeyError = String(e);
    }
  }

  async function setMode(mode: ActivationMode) {
    if (!hotkey || hotkey.mode === mode) return;
    try {
      await updateHotkey({ accelerator: hotkey.accelerator, mode });
      hotkey = { ...hotkey, mode };
    } catch (e) {
      hotkeyError = String(e);
    }
  }

  // The dictation language is read from settings at transcribe time, so persisting
  // the choice here is all that's needed — no warm-engine restart (speech-to-text.md).
  async function setLanguage(value: string) {
    if (!general) return;
    try {
      const s = await updateSettings({ general: { ...general, defaultLanguage: value as GeneralSettings["defaultLanguage"] } });
      general = s.general;
    } catch (e) {
      fail(e);
    }
  }

  async function setLaunchAtLogin(value: boolean) {
    if (!general) return;
    try {
      const s = await updateSettings({ general: { ...general, launchAtLogin: value } });
      general = s.general;
    } catch (e) {
      fail(e);
    }
  }

  async function loadModels() {
    models = await listWhisperModels();
  }

  onMount(() => {
    listInputDevices().then((d) => (devices = d)).catch(fail);
    loadModels().catch(fail);
    warmStatus().then((w) => (warm = w)).catch(fail);
    gpuEngineStatus().then((g) => (gpu = g)).catch(fail);
    getSettings().then((s) => (general = s.general)).catch(fail);
    getHotkey().then((h) => (hotkey = h)).catch(fail);
    // Auto-check for a newer signed release on launch; never throws (offline-safe).
    checkForUpdate().then((u) => (update = u)).catch(() => {});
  });

  async function download(id: string) {
    downloading = id;
    progress = 0;
    error = null;
    try {
      const channel = new Channel<DownloadProgress>();
      channel.onmessage = (p) => (progress = Math.round(p.percent));
      await downloadWhisperModel(id, channel);
      await loadModels();
    } catch (e) {
      fail(e);
    } finally {
      downloading = null;
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

  async function runMicTest() {
    micMsg = null;
    error = null;
    micTesting = true;
    try {
      const r = await testMicrophone(1500);
      micMsg =
        r.peak > 0.02
          ? `Ouvimos você (pico ${(r.peak * 100).toFixed(0)}%).`
          : "Quase nenhum som captado — verifique o microfone.";
    } catch (e) {
      fail(e);
    } finally {
      micTesting = false;
    }
  }

  async function runInjectTest() {
    injectMsg = null;
    error = null;
    try {
      await injectText(testText);
      injectMsg = "Texto enviado para a janela em foco.";
    } catch (e) {
      fail(e);
    }
  }
</script>

<main class="min-h-screen bg-cloud-mist text-midnight-indigo font-gilroy px-6 py-8">
  <div class="mx-auto flex max-w-[920px] flex-col gap-6">
    <header class="flex items-center gap-3">
      <h1 class="text-heading-lg font-bold">MIA</h1>
      <Pill tone="action">100% local · offline</Pill>
      <span class="ml-auto">
        {#if update}
          <Button disabled={updateBusy} onclick={applyUpdate}>
            {updateBusy ? "Atualizando…" : `Atualizar para v${update.version}`}
          </Button>
        {:else}
          <span class="text-body text-slate-blue">v{version}</span>
        {/if}
      </span>
    </header>

    {#if error}
      <Card class="border border-danger/30">
        <p class="text-body-lg text-danger">⚠ {error}</p>
      </Card>
    {/if}

    <Card>
      <h2 class="text-heading font-semibold">Microfone</h2>
      <p class="mt-1 text-body text-slate-blue">Escolha a entrada de áudio para a ditado.</p>
      <div class="mt-4">
        <Field label="Dispositivo de entrada" hint="Persistência da escolha chega com a captura ao vivo.">
          <select
            bind:value={selectedDevice}
            class="rounded-xl border border-platinum-tint bg-snow-white px-3 py-2 text-body-lg
                   text-midnight-indigo min-h-[40px]"
          >
            <option value="">Padrão do sistema</option>
            {#each devices as device (device.id)}
              <option value={device.id}>{device.name}{device.isDefault ? " · padrão" : ""}</option>
            {/each}
          </select>
        </Field>
      </div>
      <div class="mt-4 flex items-center gap-3">
        <Button variant="secondary" disabled={micTesting} onclick={runMicTest}>
          {micTesting ? "Ouvindo…" : "Testar microfone"}
        </Button>
        {#if micMsg}
          <span class="text-body text-slate-blue">{micMsg}</span>
        {/if}
      </div>
    </Card>

    <Card>
      <h2 class="text-heading font-semibold">Atalho (push-to-talk)</h2>
      <p class="mt-1 text-body text-slate-blue">
        Segure o atalho e fale; solte para inserir. Grave uma combinação com modificador.
      </p>
      {#if hotkeyError}
        <p class="mt-2 text-body text-danger">⚠ {hotkeyError}</p>
      {/if}
      <div class="mt-4 flex flex-wrap items-center gap-3">
        <Pill tone="neutral">{hotkey?.accelerator ?? "—"}</Pill>
        <Button variant="secondary" disabled={recording} onclick={startRecording}>
          {recording ? "Pressione a combinação…" : "Gravar atalho"}
        </Button>
        {#if recording}
          <Button variant="ghost" onclick={stopRecording}>Cancelar</Button>
        {/if}
      </div>
      <div class="mt-4">
        <Field label="Modo de ativação">
          <select
            value={hotkey?.mode ?? "pushToHold"}
            disabled={!hotkey}
            onchange={(e) => setMode((e.currentTarget as HTMLSelectElement).value as ActivationMode)}
            class="rounded-xl border border-platinum-tint bg-snow-white px-3 py-2 text-body-lg
                   text-midnight-indigo min-h-[40px]"
          >
            <option value="pushToHold">Segurar para falar</option>
            <option value="pressToToggle">Pressionar para ligar/desligar</option>
          </select>
        </Field>
      </div>
    </Card>

    <Card>
      <h2 class="text-heading font-semibold">Inicialização</h2>
      <div class="mt-3">
        {#if general}
          <Toggle
            checked={general.launchAtLogin}
            label="Abrir o MIA ao iniciar o Windows"
            onchange={setLaunchAtLogin}
          />
        {/if}
      </div>
    </Card>

    <Card>
      <h2 class="text-heading font-semibold">Idioma do ditado</h2>
      <p class="mt-1 text-body text-slate-blue">
        Automático detecta a fala; fixar pt-BR ou inglês melhora a precisão.
      </p>
      <div class="mt-4">
        <Field label="Idioma">
          <select
            value={general?.defaultLanguage ?? "auto"}
            disabled={!general}
            onchange={(e) => setLanguage((e.currentTarget as HTMLSelectElement).value)}
            class="rounded-xl border border-platinum-tint bg-snow-white px-3 py-2 text-body-lg
                   text-midnight-indigo min-h-[40px]"
          >
            <option value="auto">Automático</option>
            <option value="pt">Português (pt-BR)</option>
            <option value="en">English</option>
          </select>
        </Field>
      </div>
    </Card>

    <Card>
      <h2 class="text-heading font-semibold">Modelo Whisper</h2>
      <p class="mt-1 text-body text-slate-blue">
        Baixado sob demanda do Hugging Face — a única saída de rede do MIA.
      </p>
      <ul class="mt-4 flex flex-col gap-3">
        {#each models as model (model.id)}
          <li class="flex items-center gap-3">
            <div class="min-w-0 flex flex-1 flex-wrap items-center gap-2">
              <span class="text-body-lg font-semibold">{model.label}</span>
              <span class="text-body text-slate-blue">{model.sizeMb} MB</span>
              {#if model.recommended}<Pill tone="action">Recomendado</Pill>{/if}
            </div>
            <div class="shrink-0">
              {#if model.downloaded}
                <Pill tone="success">✓ instalado</Pill>
              {:else if downloading === model.id}
                <Pill tone="action">baixando… {progress}%</Pill>
              {:else}
                <Button variant="secondary" disabled={downloading !== null} onclick={() => download(model.id)}>
                  Baixar
                </Button>
              {/if}
            </div>
          </li>
        {/each}
      </ul>
    </Card>

    <Card>
      <h2 class="text-heading font-semibold">Motor</h2>
      <div class="mt-3 flex flex-wrap gap-3">
        <Pill tone={warm?.loaded ? "success" : "neutral"}>
          {warm?.loaded ? `quente · ${warm.model}` : "frio (nenhum modelo carregado)"}
        </Pill>
        <Pill tone="neutral">backend: {warm?.backend ?? "—"}</Pill>
        {#if gpu?.gpuPresent}
          <Pill tone={gpu.downloaded ? "success" : "action"}>
            GPU NVIDIA {gpu.downloaded ? "· engine pronto" : "· engine não baixado"}
          </Pill>
        {:else}
          <Pill tone="neutral">somente CPU</Pill>
        {/if}
      </div>
      {#if gpu?.gpuPresent && !gpu.downloaded}
        <div class="mt-4 flex items-center gap-3">
          {#if gpuDownloading}
            <Pill tone="action">baixando engine… {gpuProgress}%</Pill>
          {:else}
            <Button variant="secondary" onclick={downloadGpu}>Baixar engine GPU (~435 MB)</Button>
          {/if}
          <span class="text-body text-slate-blue">~7–10× mais rápido; troca de motor na próxima fala.</span>
        </div>
      {/if}
    </Card>

    <Card>
      <h2 class="text-heading font-semibold">Testar injeção</h2>
      <p class="mt-1 text-body text-slate-blue">
        Digita o texto na janela em foco via SendInput (ADR-005).
      </p>
      <div class="mt-4 flex flex-col gap-3">
        <input
          bind:value={testText}
          class="rounded-xl border border-platinum-tint bg-snow-white px-3 py-2 text-body-lg
                 text-midnight-indigo min-h-[40px]"
        />
        <div class="flex items-center gap-3">
          <Button onclick={runInjectTest}>Testar injeção</Button>
          {#if injectMsg}
            <span class="text-body text-success">{injectMsg}</span>
          {/if}
        </div>
      </div>
    </Card>

    <DictionarySection />
    <SnippetsSection />
  </div>
</main>
