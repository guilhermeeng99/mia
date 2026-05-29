import { invoke } from "@tauri-apps/api/core";
import type { InjectMode } from "./inject";

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
  snippetsEnabled: boolean;
  onboardingCompleted: boolean;
  toggleAutoEndpoint: boolean;
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

/** One per-app override rule; mirrors Rust `app_styles::AppStyle` (omitted = inherit). */
export interface AppStyle {
  matchExe: string;
  language?: DefaultLanguage | null;
  injectMode?: InjectMode | null;
  ensureTrailingPeriod?: boolean | null;
  spokenPunctuation?: boolean | null;
}

/** Per-app writing styles / context; mirrors Rust `settings::PerAppSettings`. */
export interface PerAppSettings {
  enabled: boolean;
  styles: AppStyle[];
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
  perApp: PerAppSettings;
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
  perApp?: PerAppSettings;
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
