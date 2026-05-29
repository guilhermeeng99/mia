<script lang="ts">
  import { Channel } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import {
    aiStatus,
    downloadLlm,
    listLlmModels,
    polish,
    runCommand,
    unloadLlm,
    type AiStatus,
    type LlmModel,
  } from "../ai";
  import { getSettings, updateSettings, type AiSettings } from "../settings";
  import type { DownloadProgress } from "../stt";
  import Button from "./ui/Button.svelte";
  import Card from "./ui/Card.svelte";
  import Field from "./ui/Field.svelte";
  import Pill from "./ui/Pill.svelte";
  import Toggle from "./ui/Toggle.svelte";

  // Optional local-LLM section (Phase 2, opt-in). Presentation only — all logic is in
  // ai_commands.rs behind the ai.ts wrappers; this never holds prompt/grammar/routing.
  let ai = $state<AiSettings | null>(null);
  let models = $state<LlmModel[]>([]);
  let status = $state<AiStatus | null>(null);
  let downloading = $state<string | null>(null);
  let progress = $state(0);
  let polishText = $state("");
  let polishOut = $state<string | null>(null);
  let cmdSpoken = $state("");
  let cmdTarget = $state("");
  let cmdOut = $state<string | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);

  function fail(e: unknown) {
    error = String(e);
    busy = false;
  }

  async function refresh() {
    models = await listLlmModels();
    status = await aiStatus();
  }

  onMount(() => {
    getSettings().then((s) => (ai = s.ai)).catch(fail);
    refresh().catch(fail);
  });

  async function setEnabled(value: boolean) {
    if (!ai) return;
    try {
      const s = await updateSettings({ ai: { ...ai, enabled: value } });
      ai = s.ai;
      await refresh();
    } catch (e) {
      fail(e);
    }
  }

  async function download(id: string) {
    downloading = id;
    progress = 0;
    error = null;
    try {
      const ch = new Channel<DownloadProgress>();
      ch.onmessage = (p) => (progress = Math.round(p.percent));
      await downloadLlm(id, ch);
      await refresh();
    } catch (e) {
      fail(e);
    } finally {
      downloading = null;
    }
  }

  async function runPolish() {
    error = null;
    polishOut = null;
    busy = true;
    try {
      polishOut = (await polish(polishText, "auto")).polishedText;
    } catch (e) {
      fail(e);
    } finally {
      busy = false;
    }
  }

  async function runCmd() {
    error = null;
    cmdOut = null;
    busy = true;
    try {
      cmdOut = (await runCommand(cmdSpoken, cmdTarget, "auto")).newText;
    } catch (e) {
      fail(e);
    } finally {
      busy = false;
    }
  }
</script>

<Card>
  <div class="flex items-center gap-3">
    <h2 class="text-heading font-semibold">IA (opcional)</h2>
    {#if ai}
      <span class="ml-auto">
        <Toggle checked={ai.enabled} label="Ativar" onchange={setEnabled} />
      </span>
    {/if}
  </div>
  <p class="mt-1 text-body text-slate-blue">
    Comandos de voz e revisão por um LLM local — 100% offline, opt-in. Nunca roda no ditado fiel.
  </p>

  {#if error}
    <p class="mt-2 text-body text-danger">⚠ {error}</p>
  {/if}

  <div class="mt-3 flex flex-wrap gap-2">
    <Pill tone={status?.enabled ? "action" : "neutral"}>{status?.enabled ? "ativado" : "desativado"}</Pill>
    <Pill tone={status?.modelInstalled ? "success" : "neutral"}>
      {status?.modelInstalled ? `modelo: ${status.modelId}` : "nenhum modelo"}
    </Pill>
    {#if status?.loaded}<Pill tone="success">quente</Pill>{/if}
  </div>

  <ul class="mt-4 flex flex-col gap-3">
    {#each models as model (model.id)}
      <li class="flex items-center gap-3">
        <div class="min-w-0 flex flex-1 flex-wrap items-center gap-2">
          <span class="text-body-lg font-semibold">{model.label}</span>
          <span class="text-body text-slate-blue">{(model.sizeMb / 1000).toFixed(1)} GB</span>
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

  {#if status?.loaded}
    <div class="mt-3"><Button variant="ghost" onclick={() => unloadLlm().then(refresh).catch(fail)}>Descarregar (liberar RAM)</Button></div>
  {/if}

  <div class="mt-5 flex flex-col gap-3 border-t border-platinum-tint pt-4">
    <Field label="Revisar (Polish)">
      <textarea
        bind:value={polishText}
        rows="2"
        placeholder="cola um texto pra revisar gramática/pontuação"
        class="rounded-xl border border-platinum-tint bg-snow-white px-3 py-2 text-body-lg text-midnight-indigo"
      ></textarea>
    </Field>
    <div class="flex items-center gap-3">
      <Button variant="secondary" disabled={busy || polishText.trim() === ""} onclick={runPolish}>Revisar</Button>
      {#if polishOut !== null}<span class="text-body text-midnight-indigo whitespace-pre-wrap">{polishOut}</span>{/if}
    </div>
  </div>

  <div class="mt-5 flex flex-col gap-3 border-t border-platinum-tint pt-4">
    <Field label="Comando falado">
      <input
        bind:value={cmdSpoken}
        placeholder="deixa isso mais formal"
        class="rounded-xl border border-platinum-tint bg-snow-white px-3 py-2 text-body-lg text-midnight-indigo min-h-[40px]"
      />
    </Field>
    <Field label="Texto alvo">
      <textarea
        bind:value={cmdTarget}
        rows="2"
        placeholder="o texto que o comando deve transformar"
        class="rounded-xl border border-platinum-tint bg-snow-white px-3 py-2 text-body-lg text-midnight-indigo"
      ></textarea>
    </Field>
    <div class="flex items-center gap-3">
      <Button variant="secondary" disabled={busy || cmdSpoken.trim() === "" || cmdTarget.trim() === ""} onclick={runCmd}>
        Executar comando
      </Button>
      {#if cmdOut !== null}<span class="text-body text-midnight-indigo whitespace-pre-wrap">{cmdOut}</span>{/if}
    </div>
  </div>
</Card>
