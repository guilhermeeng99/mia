import { invoke } from "@tauri-apps/api/core";

/** An input device for the Settings picker; mirrors Rust `audio::AudioDevice`. */
export interface AudioDevice {
  id: string;
  name: string;
  isDefault: boolean;
}

/** Enumerate input devices for the Settings picker ("System default" is flagged). */
export function listInputDevices(): Promise<AudioDevice[]> {
  return invoke<AudioDevice[]>("list_input_devices");
}
