<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import Hub from "./lib/components/Hub.svelte";

  // App.svelte stays thin: resolve the app version (the IPC smoke test) and render
  // the Settings/Hub. The floating mic HUD is a separate window (hud.rs / later).
  let version = $state("…");

  invoke<string>("app_version")
    .then((v) => (version = v))
    .catch(() => (version = "n/a"));
</script>

<Hub {version} />
