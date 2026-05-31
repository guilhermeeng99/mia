<script lang="ts">
  import { onMount } from "svelte";
  import {
    isMicPermissionDenied,
    listInputDevices,
    openMicPrivacy,
    testMicrophone,
    type AudioDevice,
  } from "../../audio";
  import { injectText } from "../../inject";
  import type { ActivationMode, HotkeyConfig } from "../../hotkey";
  import {
    getSettings,
    updateSettings,
    type AudioSettings,
    type CleanupSettings,
    type GeneralSettings,
  } from "../../settings";
  import Button from "../ui/Button.svelte";
  import Card from "../ui/Card.svelte";
  import Field from "../ui/Field.svelte";
  import PageHeader from "../ui/PageHeader.svelte";
  import Pill from "../ui/Pill.svelte";
  import Toggle from "../ui/Toggle.svelte";
  import ErrorBanner from "../ui/ErrorBanner.svelte";
  import LevelMeter from "../ui/LevelMeter.svelte";
  import { inputClass, selectClass } from "../ui/inputClass";

  // The core dictation settings view — mic input, push-to-talk binding, language,
  // startup, and the injection self-test. Presentation only; all logic goes through
  // the typed invoke() wrappers (architecture rule), never invoke() directly.
  let devices = $state<AudioDevice[]>([]);
  let selectedDevice = $state("");
  let testText = $state("Olá do MIA — teste de injeção. 🎤");
  let injectMsg = $state<string | null>(null);
  let micMsg = $state<string | null>(null);
  let micTesting = $state(false);
  let micLevel = $state(0);
  let micDenied = $state(false);
  let general = $state<GeneralSettings | null>(null);
  let audio = $state<AudioSettings | null>(null);
  let cleanup = $state<CleanupSettings | null>(null);
  let hotkey = $state<HotkeyConfig | null>(null);
  let recording = $state(false);
  let hotkeyError = $state<string | null>(null);
  let error = $state<string | null>(null);

  function fail(e: unknown) {
    error = String(e);
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

  // Persist + re-register via settings; a conflicting chord rejects before disk write.
  async function commitHotkey(accelerator: string) {
    stopRecording();
    const mode = hotkey?.mode ?? "pushToHold";
    try {
      const s = await updateSettings({ hotkey: { accelerator, mode } });
      hotkey = s.hotkey;
    } catch (e) {
      hotkeyError = String(e);
    }
  }

  async function setMode(mode: ActivationMode) {
    if (!hotkey || hotkey.mode === mode) return;
    try {
      const s = await updateSettings({ hotkey: { accelerator: hotkey.accelerator, mode } });
      hotkey = s.hotkey;
    } catch (e) {
      hotkeyError = String(e);
    }
  }

  // The dictation language is read from settings at transcribe time, so persisting
  // the choice here is all that's needed — no warm-engine restart (speech-to-text.md).
  async function setLanguage(value: string) {
    if (!general) return;
    try {
      const s = await updateSettings({
        general: { ...general, defaultLanguage: value as GeneralSettings["defaultLanguage"] },
      });
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

  async function setDictationEnabled(value: boolean) {
    if (!general) return;
    try {
      const s = await updateSettings({ general: { ...general, dictationEnabled: value } });
      general = s.general;
    } catch (e) {
      fail(e);
    }
  }

  async function setCollectStats(value: boolean) {
    if (!general) return;
    try {
      const s = await updateSettings({ general: { ...general, collectStats: value } });
      general = s.general;
    } catch (e) {
      fail(e);
    }
  }

  async function setInputDevice(value: string) {
    selectedDevice = value;
    if (!audio) return;
    try {
      const s = await updateSettings({
        audio: { ...audio, inputDevice: value || "default" },
      });
      audio = s.audio;
      selectedDevice = s.audio.inputDevice === "default" ? "" : s.audio.inputDevice;
    } catch (e) {
      fail(e);
    }
  }

  async function setCleanup<K extends keyof CleanupSettings>(key: K, value: CleanupSettings[K]) {
    if (!cleanup) return;
    try {
      const s = await updateSettings({ cleanup: { ...cleanup, [key]: value } });
      cleanup = s.cleanup;
    } catch (e) {
      fail(e);
    }
  }

  onMount(() => {
    listInputDevices().then((d) => (devices = d)).catch(fail);
    getSettings()
      .then((s) => {
        general = s.general;
        audio = s.audio;
        cleanup = s.cleanup;
        hotkey = s.hotkey;
        selectedDevice = s.audio.inputDevice === "default" ? "" : s.audio.inputDevice;
      })
      .catch(fail);
    // Always drop the global capture-phase keydown listener if the view unmounts while
    // still recording a chord (switching sidebar views destroys this component) — else
    // it leaks and keeps capturing keys against a dead component.
    return () => stopRecording();
  });

  async function runMicTest() {
    micMsg = null;
    error = null;
    micDenied = false;
    micTesting = true;
    micLevel = 0;
    try {
      const r = await testMicrophone(1500, (rms) => (micLevel = rms), selectedDevice || "default");
      micMsg =
        r.peak > 0.02
          ? `Ouvimos você (pico ${(r.peak * 100).toFixed(0)}%).`
          : "Quase nenhum som captado — verifique o microfone.";
    } catch (e) {
      micDenied = isMicPermissionDenied(String(e));
      fail(e);
    } finally {
      micTesting = false;
      micLevel = 0;
    }
  }

  function openMicSettings() {
    openMicPrivacy().catch(fail);
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

<PageHeader title="Ditado" subtitle="Como o MIA escuta e onde o texto aparece." />

<ErrorBanner message={error} />

<div class="flex flex-col gap-6">
  <Card>
    <h2 class="font-display text-title">Microfone</h2>
    <p class="mt-1 text-body text-ink-soft">Escolha a entrada de áudio para o ditado.</p>
    <div class="mt-4">
      <Field label="Dispositivo de entrada" hint="Usado no teste e no ditado ao vivo.">
        <select
          value={selectedDevice}
          onchange={(e) => setInputDevice((e.currentTarget as HTMLSelectElement).value)}
          class={selectClass}
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
      {#if micTesting}
        <LevelMeter level={micLevel} />
      {:else if micMsg}
        <span class="text-body text-ink-soft">{micMsg}</span>
      {/if}
    </div>
    {#if micDenied}
      <div class="mt-3 flex flex-wrap items-center gap-3">
        <span class="text-body text-danger">Acesso ao microfone bloqueado pelo Windows.</span>
        <Button variant="secondary" size="sm" onclick={openMicSettings}>
          Abrir configurações de microfone
        </Button>
      </div>
    {/if}
  </Card>

  <Card>
    <h2 class="font-display text-title">Atalho (push-to-talk)</h2>
    <p class="mt-1 text-body text-ink-soft">
      Segure o atalho e fale; solte para inserir. Grave uma combinação com modificador.
    </p>
    {#if hotkeyError}
      <p class="mt-2 text-body text-danger">⚠ {hotkeyError}</p>
    {/if}
    <div class="mt-4 flex flex-wrap items-center gap-3">
      <Pill tone="accent">{hotkey?.accelerator ?? "—"}</Pill>
      <Button variant="secondary" size="sm" disabled={recording} onclick={startRecording}>
        {recording ? "Pressione a combinação…" : "Gravar atalho"}
      </Button>
      {#if recording}
        <Button variant="ghost" size="sm" onclick={stopRecording}>Cancelar</Button>
      {/if}
    </div>
    <div class="mt-4">
      <Field label="Modo de ativação">
        <select
          value={hotkey?.mode ?? "pushToHold"}
          disabled={!hotkey}
          onchange={(e) => setMode((e.currentTarget as HTMLSelectElement).value as ActivationMode)}
          class={selectClass}
        >
          <option value="pushToHold">Segurar para falar</option>
          <option value="pressToToggle">Pressionar para ligar/desligar</option>
        </select>
      </Field>
    </div>
  </Card>

  <Card>
    <h2 class="font-display text-title">Idioma do ditado</h2>
    <p class="mt-1 text-body text-ink-soft">
      Automático detecta a fala; fixar pt-BR ou inglês melhora a precisão.
    </p>
    <div class="mt-4">
      <Field label="Idioma">
        <select
          value={general?.defaultLanguage ?? "auto"}
          disabled={!general}
          onchange={(e) => setLanguage((e.currentTarget as HTMLSelectElement).value)}
          class={selectClass}
        >
          <option value="auto">Automático</option>
          <option value="pt">Português (pt-BR)</option>
          <option value="en">English</option>
        </select>
      </Field>
    </div>
  </Card>

  <Card>
    <h2 class="font-display text-title">Limpeza de texto</h2>
    <div class="mt-4 grid gap-3 sm:grid-cols-2">
      {#if cleanup}
        <Toggle
          checked={cleanup.fillerRemoval}
          label="Remover vícios de fala"
          onchange={(value) => setCleanup("fillerRemoval", value)}
        />
        <Toggle
          checked={cleanup.spokenPunctuation}
          label="Converter pontuação falada"
          onchange={(value) => setCleanup("spokenPunctuation", value)}
        />
        <Toggle
          checked={cleanup.stutterCollapse}
          label="Juntar repetições"
          onchange={(value) => setCleanup("stutterCollapse", value)}
        />
        <Toggle
          checked={cleanup.capitalization}
          label="Ajustar maiúsculas"
          onchange={(value) => setCleanup("capitalization", value)}
        />
      {/if}
    </div>
  </Card>

  <Card>
    <h2 class="font-display text-title">Geral</h2>
    <div class="mt-3 flex flex-col gap-3">
      {#if general}
        <Toggle
          checked={general.dictationEnabled}
          label="Ditado ativado"
          onchange={setDictationEnabled}
        />
        <Toggle
          checked={general.collectStats}
          label="Coletar estatísticas locais"
          onchange={setCollectStats}
        />
        <Toggle
          checked={general.launchAtLogin}
          label="Abrir o MIA ao iniciar o Windows"
          onchange={setLaunchAtLogin}
        />
      {/if}
    </div>
  </Card>

  <Card>
    <h2 class="font-display text-title">Testar injeção</h2>
    <p class="mt-1 text-body text-ink-soft">
      Digita o texto na janela em foco via SendInput (ADR-005).
    </p>
    <div class="mt-4 flex flex-col gap-3">
      <input bind:value={testText} class={inputClass} />
      <div class="flex items-center gap-3">
        <Button onclick={runInjectTest}>Testar injeção</Button>
        {#if injectMsg}
          <span class="text-body text-success">{injectMsg}</span>
        {/if}
      </div>
    </div>
  </Card>
</div>
