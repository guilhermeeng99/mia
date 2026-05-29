import { invoke } from "@tauri-apps/api/core";

// Typed mirror of the Rust `settings::Settings` tree (serde camelCase). The Hub
// reads via getSettings and PATCHes whole groups via updateSettings.

export type DefaultLanguage = "auto" | "pt" | "en";
export type Engine = "cpu" | "cuda";
export type HudPosition = "caret" | "bottomCenter" | "bottomRight";
export type AiModel = "qwen2.5-3b" | "llama-3.2-3b";
export type ActivationMode = "pushToHold" | "pressToToggle";

export interface GeneralSettings {
  launchAtLogin: boolean;
  dictationEnabled: boolean;
  defaultLanguage: DefaultLanguage;
  playSounds: boolean;
  collectStats: boolean;
}

export interface HotkeyConfig {
  accelerator: string;
  mode: ActivationMode;
}

export interface ModelSettings {
  model: string;
  engine: Engine;
  unloadOnIdle: boolean;
}

export interface AudioSettings {
  inputDevice: string;
}

export interface CleanupSettings {
  fillerRemoval: boolean;
  spokenPunctuation: boolean;
  stutterCollapse: boolean;
  capitalization: boolean;
}

export interface HudSettings {
  position: HudPosition;
}

export interface AiSettings {
  enabled: boolean;
  model: AiModel;
  polishOnInsert: boolean;
}

export interface UpdatesSettings {
  autoCheckUpdates: boolean;
}

export interface Settings {
  schemaVersion: number;
  general: GeneralSettings;
  hotkey: HotkeyConfig;
  model: ModelSettings;
  audio: AudioSettings;
  cleanup: CleanupSettings;
  hud: HudSettings;
  ai: AiSettings;
  updates: UpdatesSettings;
}

/** Group-granular merge patch — send only the groups you changed. */
export interface SettingsPatch {
  general?: GeneralSettings;
  hotkey?: HotkeyConfig;
  model?: ModelSettings;
  audio?: AudioSettings;
  cleanup?: CleanupSettings;
  hud?: HudSettings;
  ai?: AiSettings;
  updates?: UpdatesSettings;
}

/** The in-memory settings (loaded once at startup; defaults if missing/corrupt). */
export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

/** Merge a patch, validate, persist atomically; resolves to the full new settings. */
export function updateSettings(patch: SettingsPatch): Promise<Settings> {
  return invoke<Settings>("update_settings", { patch });
}

/** Overwrite with defaults and persist; resolves to the defaults. */
export function resetSettings(): Promise<Settings> {
  return invoke<Settings>("reset_settings");
}
