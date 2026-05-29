import { invoke } from "@tauri-apps/api/core";

/** Backend selection for {@link injectText}; mirrors Rust `inject::InjectMode`. */
export type InjectMode = "auto" | "sendInput" | "clipboard";

/**
 * Inject `text` into the OS-focused window via the Rust engine (ADR-005). Used by
 * the Hub "test injection" action only — live dictation injects in Rust and never
 * round-trips through the webview. `mode` defaults to `auto` on the engine side.
 */
export function injectText(text: string, mode?: InjectMode): Promise<void> {
  return invoke<void>("inject_text", { text, mode });
}
