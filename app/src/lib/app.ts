import { invoke } from "@tauri-apps/api/core";

/** Running app version, compiled in from Cargo. Typed wrapper for `app_version`. */
export function appVersion(): Promise<string> {
  return invoke<string>("app_version");
}
