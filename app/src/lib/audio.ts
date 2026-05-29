import { invoke } from "@tauri-apps/api/core";

/** An input device for the Settings picker; mirrors Rust `audio::AudioDevice`. */
export interface AudioDevice {
  id: string;
  name: string;
  isDefault: boolean;
}

/** One-shot mic-test result; mirrors Rust `audio::MicTest`. */
export interface MicTest {
  peak: number;
  rms: number;
  deviceName: string;
}

/** Enumerate input devices for the Settings picker ("System default" is flagged). */
export function listInputDevices(): Promise<AudioDevice[]> {
  return invoke<AudioDevice[]>("list_input_devices");
}

/** Capture briefly from the default mic and report peak/RMS — "we can hear you" (no STT). */
export function testMicrophone(ms?: number): Promise<MicTest> {
  return invoke<MicTest>("test_microphone", { ms });
}
