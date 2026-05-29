import { invoke } from "@tauri-apps/api/core";

// Typed mirror of Rust `dictionary::{DictEntry, DictSettings}` (serde camelCase).
// The Hub holds no matching logic — it only CRUDs entries via these wrappers.

export interface DictEntry {
  id: string;
  replacement: string;
  soundsLike: string[];
  caseSensitive: boolean;
  wholeWord: boolean;
  fuzzy: boolean;
  biasPrompt: boolean;
  enabled: boolean;
}

export interface DictSettings {
  fuzzyEnabledGlobally: boolean;
  fuzzyMaxDistance: number;
  biasEnabled: boolean;
  biasMaxTerms: number;
}

/** List all dictionary entries. */
export function dictList(): Promise<DictEntry[]> {
  return invoke<DictEntry[]>("dict_list");
}

/** Add an entry (validates, rejects duplicate variants, assigns an id). */
export function dictAdd(entry: DictEntry): Promise<DictEntry> {
  return invoke<DictEntry>("dict_add", { entry });
}

/** Update an entry by id. */
export function dictUpdate(entry: DictEntry): Promise<DictEntry> {
  return invoke<DictEntry>("dict_update", { entry });
}

/** Remove an entry by id. */
export function dictRemove(id: string): Promise<void> {
  return invoke<void>("dict_remove", { id });
}

/** Read the global dictionary settings (fuzzy / bias). */
export function dictSettingsGet(): Promise<DictSettings> {
  return invoke<DictSettings>("dict_settings_get");
}

/** Replace the global dictionary settings. */
export function dictSettingsSet(settings: DictSettings): Promise<DictSettings> {
  return invoke<DictSettings>("dict_settings_set", { settings });
}
