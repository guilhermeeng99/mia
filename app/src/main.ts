import { mount } from "svelte";
// Bundled offline (no CDN, privacy-first) — the families behind --font-display
// (Alfa Slab One) and --font-body (Manrope). See docs/specs/design-system.md.
import "@fontsource/alfa-slab-one/400.css";
import "@fontsource/manrope/400.css";
import "@fontsource/manrope/500.css";
import "@fontsource/manrope/700.css";
import "@fontsource/manrope/800.css";
import "./styles.css";
import App from "./App.svelte";

// Mark the HUD window on <html> + <body> BEFORE mount so the transparent overlay
// never flashes the blush page background (a beige square) while idle.
if (new URLSearchParams(location.search).get("win") === "hud") {
  document.documentElement.classList.add("hud");
  document.body.classList.add("hud");
}

const target = document.getElementById("app");
if (!target) throw new Error("Missing #app root element");

const app = mount(App, { target });

export default app;
