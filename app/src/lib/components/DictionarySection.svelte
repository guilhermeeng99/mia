<script lang="ts">
  import { onMount } from "svelte";
  import {
    dictAdd,
    dictList,
    dictRemove,
    dictUpdate,
    type DictEntry,
  } from "../dictionary";
  import { i18n } from "../i18n";
  import Button from "./ui/Button.svelte";
  import Card from "./ui/Card.svelte";
  import Field from "./ui/Field.svelte";
  import PageHeader from "./ui/PageHeader.svelte";
  import Pill from "./ui/Pill.svelte";
  import ErrorBanner from "./ui/ErrorBanner.svelte";
  import { inputClass } from "./ui/inputClass";

  // Custom-dictionary CRUD section. Presentation only — all logic via dictionary.ts.
  let entries = $state<DictEntry[]>([]);
  let replacement = $state("");
  let sounds = $state("");
  let error = $state<string | null>(null);

  function fail(e: unknown) {
    error = String(e);
  }

  async function reload() {
    entries = await dictList();
  }

  onMount(() => {
    reload().catch(fail);
  });

  async function add() {
    error = null;
    const entry: DictEntry = {
      id: "",
      replacement: replacement.trim(),
      soundsLike: sounds
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean),
      caseSensitive: false,
      wholeWord: true,
      fuzzy: false,
      biasPrompt: true,
      enabled: true,
    };
    try {
      await dictAdd(entry);
      replacement = "";
      sounds = "";
      await reload();
    } catch (e) {
      fail(e);
    }
  }

  async function remove(id: string) {
    error = null;
    try {
      await dictRemove(id);
      await reload();
    } catch (e) {
      fail(e);
    }
  }

  async function toggle(entry: DictEntry) {
    error = null;
    try {
      await dictUpdate({ ...entry, enabled: !entry.enabled });
      await reload();
    } catch (e) {
      fail(e);
    }
  }
</script>

<PageHeader title={$i18n.dictionary.title} subtitle={$i18n.dictionary.subtitle} />

<ErrorBanner message={error} />

<Card>
  <ul class="flex flex-col gap-2">
    {#each entries as entry (entry.id)}
      <li class="flex items-center gap-3 rounded-card border-2 border-charcoal bg-canvas px-4 py-2.5">
        <span class="text-body-lg font-bold">{entry.replacement}</span>
        {#if entry.soundsLike.length}
          <span class="text-body text-ink-soft">← {entry.soundsLike.join(", ")}</span>
        {/if}
        <span class="ml-auto flex items-center gap-2">
          {#if !entry.enabled}<Pill tone="neutral">{$i18n.dictionary.disabled}</Pill>{/if}
          <Button variant="ghost" size="sm" onclick={() => toggle(entry)}>
            {entry.enabled ? $i18n.dictionary.disable : $i18n.dictionary.enable}
          </Button>
          <Button variant="ghost" size="sm" onclick={() => remove(entry.id)}>{$i18n.generic.remove}</Button>
        </span>
      </li>
    {/each}
    {#if entries.length === 0}
      <li class="text-body text-ink-soft">{$i18n.dictionary.empty}</li>
    {/if}
  </ul>

  <div class="mt-5 flex flex-col gap-4 border-t-2 border-hairline pt-5 sm:flex-row sm:items-end">
    <Field class="flex-1" label={$i18n.dictionary.correctForm}>
      <input bind:value={replacement} placeholder="MIA" class={inputClass} />
    </Field>
    <Field class="flex-1" label={$i18n.dictionary.spokenVariants}>
      <input bind:value={sounds} placeholder="mia, m i a" class={inputClass} />
    </Field>
    <Button onclick={add} disabled={replacement.trim() === ""}>{$i18n.generic.add}</Button>
  </div>
</Card>
