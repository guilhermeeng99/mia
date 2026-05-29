<script lang="ts">
  import type { Snippet } from "svelte";

  // A surface card on the cloud-mist background (see design-system.md).
  interface Props {
    class?: string;
    children: Snippet;
  }
  let { class: cls = "", children }: Props = $props();
</script>

<div class="bg-snow-white rounded-2xl shadow-sm-2 p-6 {cls}">
  {@render children()}
</div>
