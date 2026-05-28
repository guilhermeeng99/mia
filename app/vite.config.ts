import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

// Tauri expects a fixed dev port and leaves the console alone so Rust logs show.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [svelte(), tailwindcss()],
  // Tauri owns the terminal; don't let Vite clear it.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    // Don't watch the Rust side from Vite.
    watch: { ignored: ["**/src-tauri/**"] },
  },
  // Only env vars prefixed thus are exposed to the client.
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    // Tauri uses a modern webview (WebView2); target evergreen.
    target: "esnext",
    minify: process.env.TAURI_ENV_DEBUG ? false : "esbuild",
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
});
