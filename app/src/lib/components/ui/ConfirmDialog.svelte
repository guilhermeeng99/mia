<script lang="ts">
  import Button from "./Button.svelte";

  interface Props {
    open?: boolean;
    title: string;
    message: string;
    confirmLabel?: string;
    cancelLabel?: string;
    confirmVariant?: "danger" | "primary" | "secondary";
    onconfirm: () => void;
    oncancel: () => void;
  }

  let {
    open = false,
    title,
    message,
    confirmLabel = "Confirmar",
    cancelLabel = "Cancelar",
    confirmVariant = "primary",
    onconfirm,
    oncancel,
  }: Props = $props();

  function handleKeydown(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === "Escape") oncancel();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if open}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-charcoal/40"
    role="presentation"
    onclick={(e) => { if (e.target === e.currentTarget) oncancel(); }}
  >
    <div
      class="w-full max-w-sm rounded-card border-2 border-charcoal bg-canvas p-6"
      role="dialog"
      aria-modal="true"
      aria-labelledby="confirm-dialog-title"
    >
      <h2 id="confirm-dialog-title" class="font-display text-title">{title}</h2>
      <p class="mt-2 text-body text-ink-soft">{message}</p>
      <div class="mt-6 flex justify-end gap-3">
        <Button variant="ghost" onclick={oncancel}>{cancelLabel}</Button>
        <Button variant={confirmVariant} onclick={onconfirm}>{confirmLabel}</Button>
      </div>
    </div>
  </div>
{/if}
