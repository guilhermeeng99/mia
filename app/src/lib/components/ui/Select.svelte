<script lang="ts">
  // Custom styled dropdown — replaces native <select> so the popup matches the
  // Lpalo design system (pill trigger, 2px charcoal border, no OS chrome).
  // Trigger inherits inputClass dimensions; popup is absolutely positioned below.
  export interface SelectOption {
    value: string;
    label: string;
  }

  interface Props {
    options: SelectOption[];
    value: string;
    onchange?: (value: string) => void;
    disabled?: boolean;
  }

  let { options, value, onchange, disabled = false }: Props = $props();

  let open = $state(false);
  let rootEl = $state<HTMLDivElement | null>(null);

  const currentLabel = $derived(options.find((o) => o.value === value)?.label ?? value);

  function toggle() {
    if (disabled) return;
    open = !open;
  }

  function select(v: string) {
    open = false;
    onchange?.(v);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && open) open = false;
  }

  function handleOutsideClick(e: MouseEvent) {
    if (open && rootEl && !rootEl.contains(e.target as Node)) open = false;
  }
</script>

<svelte:window onkeydown={handleKeydown} onclick={handleOutsideClick} />

<div bind:this={rootEl} class="relative w-full">
  <button
    type="button"
    {disabled}
    onclick={toggle}
    aria-haspopup="listbox"
    aria-expanded={open}
    class="inline-flex w-full min-h-[42px] items-center justify-between gap-2
           rounded-pill border-2 border-charcoal bg-surface px-4 py-2
           text-body-lg text-charcoal outline-none
           focus-visible:ring-4 focus-visible:ring-pumpkin/45
           disabled:cursor-not-allowed disabled:opacity-50"
  >
    <span class="truncate">{currentLabel}</span>
    <svg
      class="shrink-0 transition-transform {open ? 'rotate-180' : ''}"
      xmlns="http://www.w3.org/2000/svg"
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="3"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <polyline points="6 9 12 15 18 9" />
    </svg>
  </button>

  {#if open}
    <ul
      role="listbox"
      class="absolute left-0 top-full z-50 mt-1 w-full overflow-hidden
             rounded-card border-2 border-charcoal bg-surface"
    >
      {#each options as opt (opt.value)}
        <li role="option" aria-selected={opt.value === value}>
          <button
            type="button"
            onclick={() => select(opt.value)}
            class="w-full px-4 py-2.5 text-left text-body-lg text-charcoal hover:bg-canvas
                   {opt.value === value ? 'bg-canvas font-bold' : ''}"
          >
            {opt.label}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>
