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
  import PageHeader from "./ui/PageHeader.svelte";
  import ErrorBanner from "./ui/ErrorBanner.svelte";
  import Toggle from "./ui/Toggle.svelte";
  import { inputClass, selectClass } from "./ui/inputClass";

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
</script>

<PageHeader
  title="Estilos por app"
  subtitle="Regras por aplicativo em foco: fixar idioma, forçar área de transferência ou ponto final."
>
  {#snippet action()}
    <Toggle checked={perApp.enabled} label="Ativado" onchange={setEnabled} />
  {/snippet}
</PageHeader>

<ErrorBanner message={error} />

<Card>
  <p class="text-body text-ink-soft">
    Use parte do nome do executável (ex.:
    <code class="rounded-md border-2 border-charcoal bg-canvas px-1.5 py-0.5 font-bold">code</code>,
    <code class="rounded-md border-2 border-charcoal bg-canvas px-1.5 py-0.5 font-bold">chrome</code>,
    <code class="rounded-md border-2 border-charcoal bg-canvas px-1.5 py-0.5 font-bold">winword</code>).
  </p>

  <ul class="mt-4 flex flex-col gap-2">
    {#each perApp.styles as s (s.matchExe)}
      <li class="flex items-center gap-3 rounded-card border-2 border-charcoal bg-canvas px-4 py-2.5">
        <span class="text-body-lg font-bold">{s.matchExe}</span>
        <span class="truncate text-body text-ink-soft">→ {describe(s)}</span>
        <Button variant="ghost" size="sm" onclick={() => remove(s.matchExe)}>Remover</Button>
      </li>
    {/each}
    {#if perApp.styles.length === 0}
      <li class="text-body text-ink-soft">Nenhuma regra ainda.</li>
    {/if}
  </ul>

  <div class="mt-5 flex flex-col gap-3 border-t-2 border-hairline pt-5">
    <Field label="Executável (parte do nome)">
      <input bind:value={matchExe} placeholder="code" class={inputClass} />
    </Field>
    <div class="flex flex-col gap-3 sm:flex-row sm:flex-wrap">
      <Field class="min-w-[160px] flex-1" label="Idioma">
        <select bind:value={language} class={selectClass}>
          <option value="inherit">Herdar</option>
          <option value="auto">Automático</option>
          <option value="pt">pt-BR</option>
          <option value="en">English</option>
        </select>
      </Field>
      <Field class="min-w-[160px] flex-1" label="Inserção">
        <select bind:value={injectMode} class={selectClass}>
          <option value="inherit">Herdar</option>
          <option value="auto">Automático</option>
          <option value="sendInput">Digitar (SendInput)</option>
          <option value="clipboard">Área de transferência</option>
        </select>
      </Field>
      <Field class="min-w-[160px] flex-1" label="Ponto final">
        <select bind:value={trailingPeriod} class={selectClass}>
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
