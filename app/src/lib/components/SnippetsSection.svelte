<script lang="ts">
  import { onMount } from "svelte";
  import {
    deleteSnippet,
    listSnippets,
    previewExpansion,
    upsertSnippet,
    type Snippet,
  } from "../snippets";
  import { getSettings, updateSettings, type GeneralSettings } from "../settings";
  import Button from "./ui/Button.svelte";
  import Card from "./ui/Card.svelte";
  import Field from "./ui/Field.svelte";
  import Toggle from "./ui/Toggle.svelte";
  import { inputClass } from "./ui/inputClass";

  // Snippets CRUD + preview section. Presentation only — all logic via snippets.ts.
  let snippets = $state<Snippet[]>([]);
  let trigger = $state("");
  let expansion = $state("");
  let sample = $state("");
  let preview = $state<string | null>(null);
  let general = $state<GeneralSettings | null>(null);
  let error = $state<string | null>(null);

  function fail(e: unknown) {
    error = String(e);
  }

  async function reload() {
    snippets = await listSnippets();
  }

  // Master switch — expansion is skipped in the pipeline when off (settings.general).
  async function setEnabled(value: boolean) {
    if (!general) return;
    try {
      const s = await updateSettings({ general: { ...general, snippetsEnabled: value } });
      general = s.general;
    } catch (e) {
      fail(e);
    }
  }

  onMount(() => {
    reload().catch(fail);
    getSettings().then((s) => (general = s.general)).catch(fail);
  });

  async function add() {
    error = null;
    const snippet: Snippet = {
      id: "",
      trigger: trigger.trim(),
      expansion,
      anchor: "anywhere",
      case: "verbatim",
      enabled: true,
    };
    try {
      await upsertSnippet(snippet);
      trigger = "";
      expansion = "";
      await reload();
    } catch (e) {
      fail(e);
    }
  }

  async function remove(id: string) {
    error = null;
    try {
      await deleteSnippet(id);
      await reload();
    } catch (e) {
      fail(e);
    }
  }

  async function runPreview() {
    error = null;
    try {
      preview = (await previewExpansion(sample)).output;
    } catch (e) {
      fail(e);
    }
  }
</script>

<Card>
  <div class="flex items-center gap-3">
    <h2 class="text-heading font-semibold">Snippets</h2>
    {#if general}
      <span class="ml-auto">
        <Toggle checked={general.snippetsEnabled} label="Ativado" onchange={setEnabled} />
      </span>
    {/if}
  </div>
  <p class="mt-1 text-body text-slate-blue">
    Frases-gatilho que expandem em texto pronto (assinatura, endereço, links).
  </p>

  {#if error}
    <p class="mt-2 text-body text-danger">⚠ {error}</p>
  {/if}

  <ul class="mt-4 flex flex-col gap-2">
    {#each snippets as s (s.id)}
      <li class="flex items-center gap-3">
        <span class="text-body-lg font-semibold">{s.trigger}</span>
        <span class="text-body text-slate-blue truncate">→ {s.expansion}</span>
        <Button variant="ghost" onclick={() => remove(s.id)}>Remover</Button>
      </li>
    {/each}
    {#if snippets.length === 0}
      <li class="text-body text-slate-blue">Nenhum snippet ainda.</li>
    {/if}
  </ul>

  <div class="mt-5 flex flex-col gap-3">
    <Field label="Gatilho">
      <input
        bind:value={trigger}
        placeholder="minha assinatura"
        class={inputClass}
      />
    </Field>
    <Field label="Expansão">
      <textarea
        bind:value={expansion}
        rows="3"
        placeholder={"João Silva\nCEO — exemplo.com"}
        class="rounded-xl border border-platinum-tint bg-snow-white px-3 py-2 text-body-lg
               text-midnight-indigo"
      ></textarea>
    </Field>
    <div>
      <Button onclick={add} disabled={trigger.trim() === "" || expansion.trim() === ""}>
        Adicionar snippet
      </Button>
    </div>
  </div>

  <div class="mt-5 flex flex-col gap-2 border-t border-platinum-tint pt-4">
    <Field label="Testar expansão">
      <input
        bind:value={sample}
        placeholder="segue minha assinatura"
        class={inputClass}
      />
    </Field>
    <div class="flex items-center gap-3">
      <Button variant="secondary" onclick={runPreview}>Pré-visualizar</Button>
      {#if preview !== null}
        <span class="text-body text-midnight-indigo whitespace-pre-wrap">{preview}</span>
      {/if}
    </div>
  </div>
</Card>
