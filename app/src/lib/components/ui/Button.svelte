<script lang="ts">
  import type { Snippet } from "svelte";

  // Shared design-system button — "Blush Playground" (Lpalo, design-system.md).
  // Pill shape, 2px charcoal outline, NO shadow; elevation comes from a hover
  // lift. Primary = charcoal fill (the unambiguous action); pumpkin is reserved
  // for active navigation. ≥40px hit target. Presentation only.
  interface Props {
    variant?: "primary" | "secondary" | "ghost" | "danger";
    size?: "md" | "sm";
    type?: "button" | "submit";
    disabled?: boolean;
    onclick?: () => void;
    children: Snippet;
  }
  let {
    variant = "primary",
    size = "md",
    type = "button",
    disabled = false,
    onclick,
    children,
  }: Props = $props();

  const base =
    "inline-flex items-center justify-center gap-2 rounded-pill border-2 font-body font-bold " +
    "transition-[transform,background-color,color,border-color] outline-none " +
    "hover:-translate-y-0.5 active:translate-y-0 " +
    "focus-visible:ring-4 focus-visible:ring-pumpkin/45 " +
    "disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:translate-y-0";
  const sizes: Record<NonNullable<Props["size"]>, string> = {
    md: "px-5 min-h-[42px] text-body-lg",
    sm: "px-4 min-h-[40px] text-body",
  };
  const variants: Record<NonNullable<Props["variant"]>, string> = {
    primary: "bg-charcoal text-surface border-charcoal",
    secondary: "bg-surface text-charcoal border-charcoal hover:bg-canvas",
    ghost: "bg-transparent text-charcoal border-transparent hover:border-charcoal hover:bg-canvas-deep",
    danger: "bg-surface text-danger border-danger hover:bg-danger hover:text-surface",
  };
</script>

<button {type} {disabled} {onclick} class="{base} {sizes[size]} {variants[variant]}">
  {@render children()}
</button>
