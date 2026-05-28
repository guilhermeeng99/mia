<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  // Scaffold smoke test: prove the Svelte ↔ Rust IPC bridge works end-to-end.
  // Real Hub UI (tabs, settings, stats) lands in later Phase 1/4 stages.
  let version = $state("…");

  invoke<string>("app_version")
    .then((v) => (version = v))
    .catch(() => (version = "n/a"));
</script>

<main class="min-h-screen bg-cloud-mist text-midnight-indigo font-gilroy px-6 py-8">
  <header class="mx-auto max-w-[920px]">
    <div class="flex items-center gap-3">
      <h1 class="text-heading-lg font-bold">MIA</h1>
      <span
        class="rounded-full bg-cloud-mist text-slate-blue text-body font-semibold px-2 py-1 border border-platinum-tint"
      >
        100% local · offline
      </span>
    </div>
    <p class="text-slate-blue text-body-lg mt-1">
      Local voice-to-text dictation for Windows — v{version}
    </p>
  </header>

  <section class="mx-auto max-w-[920px] mt-8">
    <div class="bg-snow-white rounded-2xl shadow-sm-2 p-6">
      <h2 class="text-heading font-semibold">Scaffold ready</h2>
      <p class="text-slate-blue text-body-lg mt-2">
        Tauri 2 + Svelte 5 + Vite + Tailwind v4. The dictation engine
        (capture → VAD → STT → cleanup → inject) is wired in the following
        Phase&nbsp;1 stages.
      </p>
    </div>
  </section>
</main>
