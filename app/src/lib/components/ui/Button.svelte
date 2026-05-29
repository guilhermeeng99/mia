<script lang="ts">
  import type { Snippet } from "svelte";

  // Shared design-system button (see docs/specs/design-system.md). One action
  // color (action-blue); ≥40px hit target. Presentation only.
  interface Props {
    variant?: "primary" | "secondary" | "ghost" | "danger";
    type?: "button" | "submit";
    disabled?: boolean;
    onclick?: () => void;
    children: Snippet;
  }
  let { variant = "primary", type = "button", disabled = false, onclick, children }: Props =
    $props();

  const base =
    "inline-flex items-center justify-center gap-2 rounded-xl px-4 py-2 min-h-[40px] " +
    "text-body-lg font-semibold transition-[filter,background-color,border-color] " +
    "disabled:opacity-50 disabled:cursor-not-allowed";
  const variants: Record<NonNullable<Props["variant"]>, string> = {
    primary: "bg-action-blue text-snow-white hover:brightness-110",
    secondary: "bg-snow-white text-midnight-indigo border border-platinum-tint hover:border-steel-gray",
    ghost: "bg-transparent text-action-blue hover:bg-cloud-mist",
    danger: "bg-danger text-snow-white hover:brightness-110",
  };
</script>

<button {type} {disabled} {onclick} class="{base} {variants[variant]}">
  {@render children()}
</button>
