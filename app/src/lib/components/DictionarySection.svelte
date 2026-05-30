<script lang="ts">
  import { onMount } from "svelte";
  import {
    dictAdd,
    dictList,
    dictRemove,
    dictUpdate,
    type DictEntry,
  } from "../dictionary";
  import Button from "./ui/Button.svelte";
  import Card from "./ui/Card.svelte";
  import Field from "./ui/Field.svelte";
  import Pill from "./ui/Pill.svelte";
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

<Card>
  <h2 class="text-heading font-semibold">Dicionário pessoal</h2>
  <p class="mt-1 text-body text-slate-blue">
    Nomes, jargões e siglas escritos do seu jeito (ex.: mia → MIA).
  </p>

  {#if error}
    <p class="mt-2 text-body text-danger">⚠ {error}</p>
  {/if}

  <ul class="mt-4 flex flex-col gap-2">
    {#each entries as entry (entry.id)}
      <li class="flex items-center gap-3">
        <span class="text-body-lg font-semibold">{entry.replacement}</span>
        {#if entry.soundsLike.length}
          <span class="text-body text-slate-blue">← {entry.soundsLike.join(", ")}</span>
        {/if}
        <span class="ml-auto flex items-center gap-2">
          {#if !entry.enabled}<Pill tone="neutral">desligado</Pill>{/if}
          <Button variant="ghost" onclick={() => toggle(entry)}>
            {entry.enabled ? "Desativar" : "Ativar"}
          </Button>
          <Button variant="ghost" onclick={() => remove(entry.id)}>Remover</Button>
        </span>
      </li>
    {/each}
    {#if entries.length === 0}
      <li class="text-body text-slate-blue">Nenhum termo ainda.</li>
    {/if}
  </ul>

  <div class="mt-5 flex flex-col gap-3 sm:flex-row sm:items-end">
    <Field label="Forma correta">
      <input
        bind:value={replacement}
        placeholder="MIA"
        class={inputClass}
      />
    </Field>
    <Field label="Variantes faladas (vírgula)" hint="opcional">
      <input
        bind:value={sounds}
        placeholder="mia, m i a"
        class={inputClass}
      />
    </Field>
    <Button onclick={add} disabled={replacement.trim() === ""}>Adicionar</Button>
  </div>
</Card>
