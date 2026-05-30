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

/** = unregister + register; persists only when the OS accepts the new chord. */
export function updateHotkey(cfg: HotkeyConfig): Promise<void> {
  return invoke<void>("update_hotkey", { cfg });
}
