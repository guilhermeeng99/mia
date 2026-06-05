<script lang="ts">
  import { onMount } from "svelte";
  import {
    deleteSnippet,
    listSnippets,
    previewExpansion,
    upsertSnippet,
    type Snippet,
  } from "../snippets";
  import { i18n } from "../i18n";
  import { getSettings, updateSettings, type GeneralSettings } from "../settings";
  import Button from "./ui/Button.svelte";
  import Card from "./ui/Card.svelte";
  import Field from "./ui/Field.svelte";
  import PageHeader from "./ui/PageHeader.svelte";
  import ErrorBanner from "./ui/ErrorBanner.svelte";
  import Toggle from "./ui/Toggle.svelte";
  import { inputClass, textareaClass } from "./ui/inputClass";

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

<PageHeader
  title={$i18n.snippets.title}
  subtitle={$i18n.snippets.subtitle}
>
  {#snippet action()}
    {#if general}
      <Toggle checked={general.snippetsEnabled} label={$i18n.generic.enabled} onchange={setEnabled} />
    {/if}
  {/snippet}
</PageHeader>

<ErrorBanner message={error} />

<Card>
  <ul class="flex flex-col gap-2">
    {#each snippets as s (s.id)}
      <li class="flex items-center gap-3 rounded-card border-2 border-charcoal bg-canvas px-4 py-2.5">
        <span class="text-body-lg font-bold">{s.trigger}</span>
        <span class="truncate text-body text-ink-soft">→ {s.expansion}</span>
        <Button variant="ghost" size="sm" onclick={() => remove(s.id)}>{$i18n.generic.remove}</Button>
      </li>
    {/each}
    {#if snippets.length === 0}
      <li class="text-body text-ink-soft">{$i18n.snippets.empty}</li>
    {/if}
  </ul>

  <div class="mt-5 flex flex-col gap-3 border-t-2 border-hairline pt-5">
    <Field label={$i18n.snippets.trigger}>
      <input bind:value={trigger} placeholder={$i18n.snippets.triggerPlaceholder} class={inputClass} />
    </Field>
    <Field label={$i18n.snippets.expansion}>
      <textarea
        bind:value={expansion}
        rows="3"
        placeholder={$i18n.snippets.expansionPlaceholder}
        class={textareaClass}
      ></textarea>
    </Field>
    <div>
      <Button onclick={add} disabled={trigger.trim() === "" || expansion.trim() === ""}>
        {$i18n.snippets.addSnippet}
      </Button>
    </div>
  </div>

  <div class="mt-5 flex flex-col gap-2 border-t-2 border-hairline pt-5">
    <Field label={$i18n.snippets.testExpansion}>
      <input bind:value={sample} placeholder={$i18n.snippets.samplePlaceholder} class={inputClass} />
    </Field>
    <div class="flex items-center gap-3">
      <Button variant="secondary" onclick={runPreview}>{$i18n.snippets.preview}</Button>
      {#if preview !== null}
        <span class="whitespace-pre-wrap text-body text-charcoal">{preview}</span>
      {/if}
    </div>
  </div>
</Card>
