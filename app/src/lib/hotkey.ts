import { invoke } from "@tauri-apps/api/core";

// Typed mirror of Rust `hotkey::{HotkeyConfig, ActivationMode}` (serde camelCase).

export type ActivationMode = "pushToHold" | "pressToToggle";

export interface HotkeyConfig {
  accelerator: string;
  mode: ActivationMode;
}

/** The active PTT chord + mode. */
export function getHotkey(): Promise<HotkeyConfig> {
  return invoke<HotkeyConfig>("get_hotkey");
}

/** Register `cfg`'s chord, replacing any prior registration. */
export function registerHotkey(cfg: HotkeyConfig): Promise<void> {
  return invoke<void>("register_hotkey", { cfg });
}

/** Unregister the active chord (idempotent). */
export function unregisterHotkey(): Promise<void> {
  return invoke<void>("unregister_hotkey");
}

/** = unregister + register; persists only when the OS accepts the new chord. */
export function updateHotkey(cfg: HotkeyConfig): Promise<void> {
  return invoke<void>("update_hotkey", { cfg });
}
