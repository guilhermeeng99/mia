<script lang="ts">
  // Shared danger banner (design-system.md danger token) so the six views can't drift
  // on the error-surface styling. Renders nothing when `message` is null/empty.
  interface Props {
    message?: string | null;
    class?: string;
  }
  let { message = null, class: cls = "mb-6" }: Props = $props();
</script>

{#if message}
  <div class="rounded-card border-2 border-danger bg-surface px-4 py-3 {cls}" role="alert">
    <p class="text-body-lg text-danger">⚠ {message}</p>
  </div>
{/if}
