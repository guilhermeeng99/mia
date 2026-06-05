<script lang="ts">
  import { onMount } from "svelte";
  import { i18n, perAppLanguageOptions } from "../i18n";
  import type { InjectMode } from "../inject";
  import {
    getSettings,
    updateSettings,
    type AppStyle,
    type DefaultLanguage,
    type PerAppSettings,
  } from "../settings";
  import Button from "./ui/Button.svelte";
  import Card from "./ui/Card.svelte";
  import Field from "./ui/Field.svelte";
  import PageHeader from "./ui/PageHeader.svelte";
  import ErrorBanner from "./ui/ErrorBanner.svelte";
  import Toggle from "./ui/Toggle.svelte";
  import Select from "./ui/Select.svelte";
  import { inputClass } from "./ui/inputClass";

  // Per-app writing styles / context (per-app-context.md). Presentation only — the whole
  // `perApp` group is PATCHed via updateSettings (group-granular, like cleanup/snippets).
  let perApp = $state<PerAppSettings>({ enabled: false, styles: [] });
  let matchExe = $state("");
  let language = $state<"inherit" | DefaultLanguage>("inherit");
  let injectMode = $state<"inherit" | InjectMode>("inherit");
  let trailingPeriod = $state<"inherit" | "on" | "off">("inherit");
  let error = $state<string | null>(null);

  function fail(e: unknown) {
    error = String(e);
  }

  onMount(() => {
    getSettings().then((s) => (perApp = s.perApp)).catch(fail);
  });

  async function save(next: PerAppSettings) {
    error = null;
    try {
      const s = await updateSettings({ perApp: next });
      perApp = s.perApp;
    } catch (e) {
      fail(e);
    }
  }

  function setEnabled(value: boolean) {
    void save({ ...perApp, enabled: value });
  }

  async function add() {
    const exe = matchExe.trim();
    if (exe === "") return;
    const style: AppStyle = {
      matchExe: exe,
      language: language === "inherit" ? null : language,
      injectMode: injectMode === "inherit" ? null : injectMode,
      ensureTrailingPeriod: trailingPeriod === "inherit" ? null : trailingPeriod === "on",
      spokenPunctuation: null,
    };
    await save({ ...perApp, styles: [...perApp.styles, style] });
    matchExe = "";
    language = "inherit";
    injectMode = "inherit";
    trailingPeriod = "inherit";
  }

  function remove(exe: string) {
    void save({ ...perApp, styles: perApp.styles.filter((s) => s.matchExe !== exe) });
  }

  function describe(s: AppStyle): string {
    const parts: string[] = [];
    if (s.language) parts.push(languageLabel(s.language));
    if (s.injectMode) parts.push(injectModeLabel(s.injectMode));
    if (s.ensureTrailingPeriod != null) {
      parts.push(s.ensureTrailingPeriod ? $i18n.perApp.finalPeriod : $i18n.perApp.noPeriod);
    }
    return parts.length ? parts.join(" · ") : $i18n.perApp.noChanges;
  }

  function languageLabel(value: DefaultLanguage): string {
    return perAppLanguageOptions($i18n).find((option) => option.value === value)?.label ?? value;
  }

  function injectModeLabel(value: InjectMode): string {
    if (value === "sendInput") return $i18n.perApp.sendInput;
    if (value === "clipboard") return $i18n.perApp.clipboard;
    return $i18n.perApp.automatic;
  }
</script>

<PageHeader
  title={$i18n.perApp.title}
  subtitle={$i18n.perApp.subtitle}
>
  {#snippet action()}
    <Toggle checked={perApp.enabled} label={$i18n.generic.enabled} onchange={setEnabled} />
  {/snippet}
</PageHeader>

<ErrorBanner message={error} />

<Card>
  <p class="text-body text-ink-soft">
    {$i18n.perApp.description}
    <code class="rounded-md border-2 border-charcoal bg-canvas px-1.5 py-0.5 font-bold">code</code>,
    <code class="rounded-md border-2 border-charcoal bg-canvas px-1.5 py-0.5 font-bold">chrome</code>,
    <code class="rounded-md border-2 border-charcoal bg-canvas px-1.5 py-0.5 font-bold">winword</code>).
  </p>

  <ul class="mt-4 flex flex-col gap-2">
    {#each perApp.styles as s (s.matchExe)}
      <li class="flex items-center gap-3 rounded-card border-2 border-charcoal bg-canvas px-4 py-2.5">
        <span class="text-body-lg font-bold">{s.matchExe}</span>
        <span class="truncate text-body text-ink-soft">→ {describe(s)}</span>
        <Button variant="ghost" size="sm" onclick={() => remove(s.matchExe)}>{$i18n.generic.remove}</Button>
      </li>
    {/each}
    {#if perApp.styles.length === 0}
      <li class="text-body text-ink-soft">{$i18n.perApp.empty}</li>
    {/if}
  </ul>

  <div class="mt-5 flex flex-col gap-3 border-t-2 border-hairline pt-5">
    <Field label={$i18n.perApp.executable}>
      <input bind:value={matchExe} placeholder="code" class={inputClass} />
    </Field>
    <div class="flex flex-col gap-3 sm:flex-row sm:flex-wrap">
      <Field class="min-w-[160px] flex-1" label={$i18n.perApp.language}>
        <Select
          options={perAppLanguageOptions($i18n)}
          value={language}
          onchange={(v) => { language = v as typeof language; }}
        />
      </Field>
      <Field class="min-w-[160px] flex-1" label={$i18n.perApp.insertion}>
        <Select
          options={[
            { value: "inherit", label: $i18n.perApp.inherit },
            { value: "auto", label: $i18n.perApp.automatic },
            { value: "sendInput", label: $i18n.perApp.sendInput },
            { value: "clipboard", label: $i18n.perApp.clipboard },
          ]}
          value={injectMode}
          onchange={(v) => { injectMode = v as typeof injectMode; }}
        />
      </Field>
      <Field class="min-w-[160px] flex-1" label={$i18n.perApp.trailingPeriod}>
        <Select
          options={[
            { value: "inherit", label: $i18n.perApp.inherit },
            { value: "on", label: $i18n.perApp.always },
            { value: "off", label: $i18n.perApp.never },
          ]}
          value={trailingPeriod}
          onchange={(v) => { trailingPeriod = v as typeof trailingPeriod; }}
        />
      </Field>
    </div>
    <div>
      <Button onclick={add} disabled={matchExe.trim() === ""}>{$i18n.perApp.addRule}</Button>
    </div>
  </div>
</Card>
