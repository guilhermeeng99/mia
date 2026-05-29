<script lang="ts">
  import { onMount } from "svelte";
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
  import Toggle from "./ui/Toggle.svelte";

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
    if (s.language) parts.push(s.language === "pt" ? "pt-BR" : s.language === "en" ? "English" : "auto");
    if (s.injectMode) parts.push(s.injectMode);
    if (s.ensureTrailingPeriod != null) parts.push(s.ensureTrailingPeriod ? "ponto final" : "sem ponto");
    return parts.length ? parts.join(" · ") : "sem alterações";
  }

  const inputClass =
    "rounded-xl border border-platinum-tint bg-snow-white px-3 py-2 text-body-lg text-midnight-indigo min-h-[40px]";
</script>

<Card>
  <div class="flex items-center gap-3">
    <h2 class="text-heading font-semibold">Estilos por app</h2>
    <span class="ml-auto">
      <Toggle checked={perApp.enabled} label="Ativado" onchange={setEnabled} />
    </span>
  </div>
  <p class="mt-1 text-body text-slate-blue">
    Regras por aplicativo em foco (ex.: <code>code</code>, <code>chrome</code>, <code>winword</code>):
    fixar idioma, forçar área de transferência ou ponto final.
  </p>

  {#if error}
    <p class="mt-2 text-body text-danger">⚠ {error}</p>
  {/if}

  <ul class="mt-4 flex flex-col gap-2">
    {#each perApp.styles as s (s.matchExe)}
      <li class="flex items-center gap-3">
        <span class="text-body-lg font-semibold">{s.matchExe}</span>
        <span class="text-body text-slate-blue truncate">→ {describe(s)}</span>
        <Button variant="ghost" onclick={() => remove(s.matchExe)}>Remover</Button>
      </li>
    {/each}
    {#if perApp.styles.length === 0}
      <li class="text-body text-slate-blue">Nenhuma regra ainda.</li>
    {/if}
  </ul>

  <div class="mt-5 flex flex-col gap-3">
    <Field label="Executável (parte do nome)">
      <input bind:value={matchExe} placeholder="code" class={inputClass} />
    </Field>
    <div class="flex flex-wrap gap-3">
      <Field label="Idioma">
        <select bind:value={language} class={inputClass}>
          <option value="inherit">Herdar</option>
          <option value="auto">Automático</option>
          <option value="pt">pt-BR</option>
          <option value="en">English</option>
        </select>
      </Field>
      <Field label="Inserção">
        <select bind:value={injectMode} class={inputClass}>
          <option value="inherit">Herdar</option>
          <option value="auto">Automático</option>
          <option value="sendInput">Digitar (SendInput)</option>
          <option value="clipboard">Área de transferência</option>
        </select>
      </Field>
      <Field label="Ponto final">
        <select bind:value={trailingPeriod} class={inputClass}>
          <option value="inherit">Herdar</option>
          <option value="on">Sempre</option>
          <option value="off">Nunca</option>
        </select>
      </Field>
    </div>
    <div>
      <Button onclick={add} disabled={matchExe.trim() === ""}>Adicionar regra</Button>
    </div>
  </div>
</Card>
