<script lang="ts">
  // A sidebar navigation pill (design-system.md §8a). Active = pumpkin fill +
  // charcoal outline (the Lpalo active-nav treatment); inactive is borderless
  // until hover. Icon is decorative because the label carries the accessible name.
  interface Props {
    label: string;
    iconSrc: string;
    active?: boolean;
    onclick?: () => void;
  }
  let { label, iconSrc, active = false, onclick }: Props = $props();
</script>

<button
  type="button"
  {onclick}
  aria-current={active ? "page" : undefined}
  class="flex w-full items-center gap-3 rounded-pill border-2 px-4 py-2.5 text-left
         text-body-lg font-bold text-charcoal outline-none transition-colors
         focus-visible:ring-4 focus-visible:ring-pumpkin/45
         {active ? 'border-charcoal bg-pumpkin' : 'border-transparent hover:border-charcoal hover:bg-surface'}"
>
  <img
    src={iconSrc}
    alt=""
    aria-hidden="true"
    draggable="false"
    class="h-7 w-7 shrink-0 object-contain"
  />
  <span>{label}</span>
</button>
