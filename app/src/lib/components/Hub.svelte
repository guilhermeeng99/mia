<script lang="ts">
  import { onMount } from "svelte";
  import { checkForUpdate, installUpdate, type Update } from "../update";
  import Button from "./ui/Button.svelte";
  import NavItem from "./ui/NavItem.svelte";
  import Pill from "./ui/Pill.svelte";
  import OverviewView from "./views/OverviewView.svelte";
  import DictationView from "./views/DictationView.svelte";
  import ModelsView from "./views/ModelsView.svelte";
  import DictionarySection from "./DictionarySection.svelte";
  import SnippetsSection from "./SnippetsSection.svelte";
  import PerAppSection from "./PerAppSection.svelte";

  // The Settings/Hub shell — a left sidebar + a scrollable content area (design-system.md
  // §8a). Presentation only: it routes between self-contained views, each of which calls
  // the typed invoke() wrappers itself. The shell owns just navigation + the signed-update
  // affordance (ADR-009).
  let { version }: { version: string } = $props();

  type ViewId = "overview" | "dictation" | "models" | "dictionary" | "snippets" | "perapp";
  const NAV: { id: ViewId; label: string; icon: string }[] = [
    { id: "overview", label: "Visão geral", icon: "🏠" },
    { id: "dictation", label: "Ditado", icon: "🎙️" },
    { id: "models", label: "Modelos & Motor", icon: "🧠" },
    { id: "dictionary", label: "Dicionário", icon: "📖" },
    { id: "snippets", label: "Snippets", icon: "✂️" },
    { id: "perapp", label: "Por app", icon: "🪟" },
  ];
  let active = $state<ViewId>("overview");

  let update = $state<Update | null>(null);
  let updateBusy = $state(false);

  onMount(() => {
    // Auto-check for a newer signed release on launch; never throws (offline-safe).
    checkForUpdate().then((u) => (update = u)).catch(() => {});
  });

  async function applyUpdate() {
    if (!update) return;
    updateBusy = true;
    try {
      await installUpdate(update); // downloads, installs, relaunches
    } catch {
      updateBusy = false;
    }
  }
</script>

<div class="flex h-full overflow-hidden bg-canvas font-body text-charcoal">
  <aside class="flex w-[244px] shrink-0 flex-col border-r-2 border-charcoal bg-canvas-deep">
    <div class="flex items-center gap-3 px-5 pt-6 pb-5">
      <img src="/logo.png" alt="MIA" class="h-11 w-auto shrink-0" />
      <Pill tone="info">100% local · offline</Pill>
    </div>

    <nav class="flex flex-1 flex-col gap-1.5 px-3">
      {#each NAV as item (item.id)}
        <NavItem
          label={item.label}
          icon={item.icon}
          active={active === item.id}
          onclick={() => (active = item.id)}
        />
      {/each}
    </nav>

    <footer class="border-t-2 border-charcoal px-5 py-5">
      {#if update}
        <Button size="sm" disabled={updateBusy} onclick={applyUpdate}>
          {updateBusy ? "Atualizando…" : `Atualizar v${update.version}`}
        </Button>
      {:else}
        <span class="text-caption font-bold text-ink-soft">versão {version}</span>
      {/if}
    </footer>
  </aside>

  <main class="flex-1 overflow-y-auto">
    <div class="mx-auto max-w-[820px] px-10 py-9">
      <section hidden={active !== "overview"}>
        <OverviewView />
      </section>
      <section hidden={active !== "dictation"}>
        <DictationView />
      </section>
      <section hidden={active !== "models"}>
        <ModelsView />
      </section>
      <section hidden={active !== "dictionary"}>
        <DictionarySection />
      </section>
      <section hidden={active !== "snippets"}>
        <SnippetsSection />
      </section>
      <section hidden={active !== "perapp"}>
        <PerAppSection />
      </section>
    </div>
  </main>
</div>
