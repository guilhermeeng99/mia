import { Channel, invoke } from "@tauri-apps/api/core";

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

/** Live capture event streamed during the mic test; mirrors Rust `audio::CaptureEvent`. */
export type CaptureEvent =
  | { kind: "level"; rms: number; peak: number }
  | { kind: "error"; message: string };

/** Sentinel prefix the engine puts on a mic-permission denial (see Rust `classify_mic_error`). */
export const MIC_PERMISSION_DENIED = "mic-permission-denied";

/** True when a mic error string is a Windows permission denial — show the deep-link affordance. */
export function isMicPermissionDenied(message: string): boolean {
  return message.includes(MIC_PERMISSION_DENIED) || /permiss/i.test(message);
}

/** Enumerate input devices for the Settings picker ("System default" is flagged). */
export function listInputDevices(): Promise<AudioDevice[]> {
  return invoke<AudioDevice[]>("list_input_devices");
}

/**
 * Capture briefly from the selected mic and report peak/RMS — "we can hear you" (no STT).
 * Pass `onLevel` to receive the live RMS stream while the test runs (for a live meter).
 */
export function testMicrophone(
  ms?: number,
  onLevel?: (rms: number) => void,
  deviceId?: string,
): Promise<MicTest> {
  const level = new Channel<CaptureEvent>();
  if (onLevel) {
    level.onmessage = (e) => {
      if (e.kind === "level") onLevel(e.rms);
    };
  }
  return invoke<MicTest>("test_microphone", { ms, level, deviceId });
}

/** Open Windows' microphone privacy settings (deep-link) when capture is permission-denied. */
export function openMicPrivacy(): Promise<void> {
  return invoke<void>("open_mic_privacy");
}
