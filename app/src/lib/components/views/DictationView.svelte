<script lang="ts">
  import { onMount } from "svelte";
  import {
    isMicPermissionDenied,
    listInputDevices,
    openMicPrivacy,
    testMicrophone,
    type AudioDevice,
  } from "../../audio";
  import {
    sampleHotkeyRecording,
    unregisterHotkey,
    updateHotkey,
    type ActivationMode,
    type HotkeyConfig,
  } from "../../hotkey";
  import { dictationLanguageOptions, i18n } from "../../i18n";
  import {
    getSettings,
    updateSettings,
    type AudioSettings,
    type CleanupSettings,
    type GeneralSettings,
    type HudSettings,
    type Indicator,
  } from "../../settings";
  import Button from "../ui/Button.svelte";
  import Card from "../ui/Card.svelte";
  import Field from "../ui/Field.svelte";
  import PageHeader from "../ui/PageHeader.svelte";
  import Pill from "../ui/Pill.svelte";
  import Select from "../ui/Select.svelte";
  import Toggle from "../ui/Toggle.svelte";
  import ErrorBanner from "../ui/ErrorBanner.svelte";
  import LevelMeter from "../ui/LevelMeter.svelte";

  // The core dictation settings view — mic input, push-to-talk binding, language,
  // and startup. Presentation only; all logic goes through
  // the typed invoke() wrappers (architecture rule), never invoke() directly.
  let devices = $state<AudioDevice[]>([]);
  let selectedDevice = $state("");
  let micMsg = $state<string | null>(null);
  let micTesting = $state(false);
  let micLevel = $state(0);
  let micDenied = $state(false);
  let general = $state<GeneralSettings | null>(null);
  let hud = $state<HudSettings | null>(null);
  let audio = $state<AudioSettings | null>(null);
  let cleanup = $state<CleanupSettings | null>(null);
  let hotkey = $state<HotkeyConfig | null>(null);
  let recording = $state(false);
  let armingHotkeyRecorder = $state(false);
  let hotkeySuspended = $state(false);
  let recordingPoll = $state<number | null>(null);
  let pendingHotkey = $state<string | null>(null);
  let hotkeyError = $state<string | null>(null);
  let error = $state<string | null>(null);

  const deviceOptions = $derived([
    { value: "", label: $i18n.generic.systemDefault },
    ...devices.map((d) => ({ value: d.id, label: d.name + (d.isDefault ? ` · ${$i18n.dictation.defaultDeviceSuffix}` : "") })),
  ]);


  function fail(e: unknown) {
    error = String(e);
  }

  async function pollHotkeyRecording() {
    if (!recording) return;
    try {
      const sample = await sampleHotkeyRecording();
      if (!recording) return;
      if (sample.cancelled) {
        await cancelRecording();
        return;
      }
      if (sample.accelerator) pendingHotkey = sample.accelerator;
      if (sample.released && pendingHotkey) {
        await commitHotkey(pendingHotkey);
      }
    } catch (e) {
      hotkeyError = String(e);
      await cancelRecording();
    }
  }

  async function startRecording() {
    if (recording || armingHotkeyRecorder) return;
    hotkeyError = null;
    pendingHotkey = null;
    armingHotkeyRecorder = true;
    try {
      await unregisterHotkey();
      hotkeySuspended = true;
      recording = true;
      recordingPoll = window.setInterval(() => void pollHotkeyRecording(), 30);
      void pollHotkeyRecording();
    } catch (e) {
      hotkeyError = String(e);
    } finally {
      armingHotkeyRecorder = false;
    }
  }

  function stopRecording() {
    recording = false;
    if (recordingPoll !== null) {
      window.clearInterval(recordingPoll);
      recordingPoll = null;
    }
  }

  async function restoreCurrentHotkeyRuntime() {
    if (!hotkeySuspended || !hotkey) return;
    try {
      await updateHotkey(hotkey);
    } catch (e) {
      hotkeyError = String(e);
    } finally {
      hotkeySuspended = false;
    }
  }

  // Persist + re-register via settings; a conflicting chord rejects before disk write.
  async function commitHotkey(accelerator: string) {
    stopRecording();
    const mode = hotkey?.mode ?? "pushToHold";
    try {
      const s = await updateSettings({ hotkey: { accelerator, mode } });
      hotkey = s.hotkey;
      pendingHotkey = null;
      hotkeySuspended = false;
    } catch (e) {
      hotkeyError = String(e);
      await restoreCurrentHotkeyRuntime();
    }
  }

  async function cancelRecording() {
    stopRecording();
    pendingHotkey = null;
    hotkeyError = null;
    await restoreCurrentHotkeyRuntime();
  }

  function confirmPendingHotkey() {
    if (!pendingHotkey) return;
    void commitHotkey(pendingHotkey);
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

  async function setDictationSounds(value: boolean) {
    if (!general) return;
    try {
      const s = await updateSettings({ general: { ...general, dictationSounds: value } });
      general = s.general;
    } catch (e) {
      fail(e);
    }
  }

  async function setCopyToClipboard(value: boolean) {
    if (!general) return;
    try {
      const s = await updateSettings({ general: { ...general, copyToClipboard: value } });
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

  // The indicator choice is read by the engine per phase-change, so persisting is all that's
  // needed — no warm-engine restart.
  async function setIndicatorField(key: keyof Indicator, value: boolean) {
    if (!hud) return;
    try {
      const s = await updateSettings({ hud: { ...hud, indicator: { ...hud.indicator, [key]: value } } });
      hud = s.hud;
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
        hud = s.hud;
        selectedDevice = s.audio.inputDevice === "default" ? "" : s.audio.inputDevice;
      })
      .catch(fail);
    // Always drop the global capture-phase keydown listener if the view unmounts while
    // still recording a chord (switching sidebar views destroys this component) — else
    // it leaks and keeps capturing keys against a dead component.
    return () => {
      void cancelRecording();
    };
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
          ? $i18n.dictation.micHeard((r.peak * 100).toFixed(0))
          : $i18n.dictation.micQuiet;
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

</script>

<PageHeader title={$i18n.dictation.title} subtitle={$i18n.dictation.subtitle} />

<ErrorBanner message={error} />

<div class="flex flex-col gap-6">
  <Card>
    <h2 class="font-display text-title">{$i18n.dictation.micTitle}</h2>
    <p class="mt-1 text-body text-ink-soft">{$i18n.dictation.micSubtitle}</p>
    <div class="mt-4">
      <Field label={$i18n.dictation.inputDevice} hint={$i18n.dictation.inputHint}>
        <Select options={deviceOptions} value={selectedDevice} onchange={setInputDevice} />
      </Field>
    </div>
    <div class="mt-4 flex items-center gap-3">
      <Button variant="secondary" disabled={micTesting} onclick={runMicTest}>
        {micTesting ? $i18n.onboarding.listening : $i18n.dictation.testMic}
      </Button>
      {#if micTesting}
        <LevelMeter level={micLevel} />
      {:else if micMsg}
        <span class="text-body text-ink-soft">{micMsg}</span>
      {/if}
    </div>
    {#if micDenied}
      <div class="mt-3 flex flex-wrap items-center gap-3">
        <span class="text-body text-danger">{$i18n.dictation.micBlocked}</span>
        <Button variant="secondary" size="sm" onclick={openMicSettings}>
          {$i18n.dictation.openMicSettings}
        </Button>
      </div>
    {/if}
  </Card>

  <Card>
    <h2 class="font-display text-title">{$i18n.dictation.hotkeyTitle}</h2>
    <p class="mt-1 text-body text-ink-soft">
      {$i18n.dictation.hotkeySubtitle}
    </p>
    {#if hotkeyError}
      <p class="mt-2 text-body text-danger">⚠ {hotkeyError}</p>
    {/if}
    <div class="mt-4 flex flex-wrap items-center gap-3">
      <Pill tone="accent">{hotkey?.accelerator ?? "—"}</Pill>
      {#if pendingHotkey}
        <Pill tone="info">{$i18n.dictation.newHotkey(pendingHotkey)}</Pill>
      {/if}
      <Button variant="secondary" size="sm" disabled={recording || armingHotkeyRecorder || !!pendingHotkey} onclick={() => void startRecording()}>
        {recording ? $i18n.dictation.pressCombination : armingHotkeyRecorder ? $i18n.dictation.preparing : $i18n.dictation.recordHotkey}
      </Button>
      {#if pendingHotkey}
        <Button size="sm" onclick={confirmPendingHotkey}>{$i18n.generic.confirm}</Button>
        <Button variant="ghost" size="sm" onclick={() => void cancelRecording()}>{$i18n.generic.cancel}</Button>
      {:else if recording}
        <Button variant="ghost" size="sm" onclick={() => void cancelRecording()}>{$i18n.generic.cancel}</Button>
      {/if}
    </div>
    <div class="mt-4">
      <Field label={$i18n.dictation.activationMode}>
        <Select
          options={[
            { value: "pushToHold", label: $i18n.dictation.pushToHold },
            { value: "pressToToggle", label: $i18n.dictation.pressToToggle },
          ]}
          value={hotkey?.mode ?? "pushToHold"}
          disabled={!hotkey}
          onchange={(v) => setMode(v as ActivationMode)}
        />
      </Field>
    </div>
  </Card>

  <Card>
    <h2 class="font-display text-title">{$i18n.dictation.languageTitle}</h2>
    <p class="mt-1 text-body text-ink-soft">
      {$i18n.dictation.languageSubtitle}
    </p>
    <div class="mt-4">
      <Field label={$i18n.dictation.languageField}>
        <Select
          options={dictationLanguageOptions($i18n)}
          value={general?.defaultLanguage ?? "auto"}
          disabled={!general}
          onchange={setLanguage}
        />
      </Field>
    </div>
  </Card>

  <Card>
    <h2 class="font-display text-title">{$i18n.dictation.indicatorTitle}</h2>
    <p class="mt-1 text-body text-ink-soft">
      {$i18n.dictation.indicatorSubtitle}
    </p>
    <div class="mt-4 grid gap-3 sm:grid-cols-2">
      {#if hud}
        <Toggle
          checked={hud.indicator.overlay}
          label={$i18n.dictation.overlay}
          onchange={(value) => setIndicatorField("overlay", value)}
        />
        <Toggle
          checked={hud.indicator.tray}
          label={$i18n.dictation.tray}
          onchange={(value) => setIndicatorField("tray", value)}
        />
      {/if}
    </div>
  </Card>

  <Card>
    <h2 class="font-display text-title">{$i18n.dictation.cleanupTitle}</h2>
    <div class="mt-4 grid gap-3 sm:grid-cols-2">
      {#if cleanup}
        <Toggle
          checked={cleanup.fillerRemoval}
          label={$i18n.dictation.fillerRemoval}
          onchange={(value) => setCleanup("fillerRemoval", value)}
        />
        <Toggle
          checked={cleanup.spokenPunctuation}
          label={$i18n.dictation.spokenPunctuation}
          onchange={(value) => setCleanup("spokenPunctuation", value)}
        />
        <Toggle
          checked={cleanup.stutterCollapse}
          label={$i18n.dictation.stutterCollapse}
          onchange={(value) => setCleanup("stutterCollapse", value)}
        />
        <Toggle
          checked={cleanup.capitalization}
          label={$i18n.dictation.capitalization}
          onchange={(value) => setCleanup("capitalization", value)}
        />
      {/if}
    </div>
  </Card>

  <Card>
    <h2 class="font-display text-title">{$i18n.dictation.generalTitle}</h2>
    <div class="mt-3 flex flex-col gap-3">
      {#if general}
        <Toggle
          checked={general.launchAtLogin}
          label={$i18n.dictation.launchAtLogin}
          onchange={setLaunchAtLogin}
        />
        <Toggle
          checked={general.dictationSounds}
          label={$i18n.dictation.dictationSounds}
          onchange={setDictationSounds}
        />
        <Toggle
          checked={general.copyToClipboard}
          label={$i18n.dictation.copyToClipboard}
          onchange={setCopyToClipboard}
        />
      {/if}
    </div>
  </Card>
</div>
