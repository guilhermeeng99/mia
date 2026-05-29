//! Microphone capture front-end (ADR-001/004, Phase 1) — turns the selected input
//! device into the 16 kHz mono `s16` PCM the warm Whisper model expects, and feeds
//! the HUD level meter. See `docs/specs/audio-capture.md`.
//!
//! The DSP core here is **pure and cargo-tested** (downmix, resample, quantize,
//! RMS/peak, frame chunking, device-name normalization). The real-time cpal stream,
//! the lock-free ring buffer, and the processing/VAD thread (§5) are the runtime
//! seam wired during the orchestrator stage; this file currently also exposes the
//! device-enumeration command the Settings picker needs.

use cpal::traits::{DeviceTrait, HostTrait};
use serde::Serialize;

/// Whisper's native input rate — every frame handed to STT is at this rate (Rule 1).
pub const SAMPLE_RATE_HZ: u32 = 16_000;
/// Samples in one VAD/processing frame at the output rate (16 kHz × 30 ms = 480).
pub const FRAME_SAMPLES: usize = (SAMPLE_RATE_HZ as usize / 1000) * crate::vad::FRAME_MS as usize;

// ─────────────────────────────────────────────────────────────────────────────
// Pure DSP helpers (cargo-tested, no I/O)
// ─────────────────────────────────────────────────────────────────────────────

/// Average all channels of an interleaved buffer down to mono (Rule 1). A trailing
/// partial frame (len not a multiple of `channels`) is dropped. `channels ≤ 1` is a
/// no-op copy.
pub fn downmix_to_mono(interleaved: &[f32], channels: u16) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    if ch == 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

/// Quantize one `f32` sample to `s16`: clamp to `[-1.0, 1.0]`, scale, round (Rule 1).
pub fn f32_to_s16(x: f32) -> i16 {
    (x.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16
}

/// Resample a mono buffer with **linear interpolation** (Rule 1). WHY linear: it is
/// cheap and good enough for a VAD-gated 48→16 kHz dictation path; a low-pass /
/// `rubato` polyphase upgrade (to suppress downsampling aliasing) is a documented
/// later optimization (spec §2). Output length is `floor(len × to / from)`.
pub fn resample_linear(input: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
    if input.is_empty() || from_hz == 0 || to_hz == 0 {
        return Vec::new();
    }
    if from_hz == to_hz {
        return input.to_vec();
    }
    let out_len = (input.len() as u64 * to_hz as u64 / from_hz as u64) as usize;
    let step = from_hz as f64 / to_hz as f64;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * step;
            let idx = pos.floor() as usize;
            let frac = (pos - idx as f64) as f32;
            let a = input[idx];
            let b = *input.get(idx + 1).unwrap_or(&a);
            a + (b - a) * frac
        })
        .collect()
}

/// Root-mean-square energy of a frame on the `[0.0, 1.0]` scale (Rule 10). Empty → 0.
pub fn rms(frame: &[f32]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = frame.iter().map(|x| x * x).sum();
    (sum_sq / frame.len() as f32).sqrt()
}

/// Absolute peak of a frame on the `[0.0, 1.0]` scale (Rule 10). Empty → 0.
pub fn peak(frame: &[f32]) -> f32 {
    frame.iter().fold(0.0_f32, |m, x| m.max(x.abs()))
}

/// Splits an arbitrary stream of samples into exact fixed-size frames, buffering the
/// remainder across calls (§2 "frame chunking"). The cpal callback delivers
/// arbitrary buffer sizes; VAD needs uniform 30 ms frames.
pub struct FrameChunker {
    frame_len: usize,
    buf: Vec<f32>,
}

impl FrameChunker {
    pub fn new(frame_len: usize) -> Self {
        Self { frame_len: frame_len.max(1), buf: Vec::new() }
    }

    /// Append `samples` and return every full frame now available; the sub-frame
    /// remainder stays buffered for the next call.
    pub fn push(&mut self, samples: &[f32]) -> Vec<Vec<f32>> {
        self.buf.extend_from_slice(samples);
        let mut frames = Vec::new();
        let mut start = 0;
        while start + self.frame_len <= self.buf.len() {
            frames.push(self.buf[start..start + self.frame_len].to_vec());
            start += self.frame_len;
        }
        self.buf.drain(..start);
        frames
    }

    /// Samples currently buffered below one full frame.
    pub fn pending(&self) -> usize {
        self.buf.len()
    }
}

/// Collapse runs of whitespace in a raw cpal device name for tidy display (§2).
pub fn normalize_device_name(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ─────────────────────────────────────────────────────────────────────────────
// Device enumeration command
// ─────────────────────────────────────────────────────────────────────────────

/// An input device for the Settings picker (§2). `id` is the raw cpal name (the
/// WASAPI handle), `name` is normalized for display.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    id: String,
    name: String,
    is_default: bool,
}

/// Enumerate input devices for the Settings picker (§2). Returns an empty list if
/// the host has no inputs; surfaces a cpal enumeration failure as `Err`.
#[tauri::command]
pub fn list_input_devices() -> Result<Vec<AudioDevice>, String> {
    let host = cpal::default_host();
    let default_name = host.default_input_device().and_then(|d| d.name().ok());
    let devices = host
        .input_devices()
        .map_err(|e| format!("no input device available: {e}"))?;
    let mut out = Vec::new();
    for device in devices {
        let raw = device.name().map_err(|e| format!("failed to read device name: {e}"))?;
        let is_default = default_name.as_deref() == Some(raw.as_str());
        out.push(AudioDevice { is_default, name: normalize_device_name(&raw), id: raw });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_averages_stereo() {
        // [L,R, L,R] = [1.0,-1.0, 0.5,0.5] → [0.0, 0.5]
        assert_eq!(downmix_to_mono(&[1.0, -1.0, 0.5, 0.5], 2), vec![0.0, 0.5]);
    }

    #[test]
    fn downmix_mono_is_passthrough() {
        assert_eq!(downmix_to_mono(&[0.1, 0.2, 0.3], 1), vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn downmix_drops_trailing_partial_frame() {
        // 3 samples, 2 channels → one full pair, the lone trailing sample is dropped.
        assert_eq!(downmix_to_mono(&[1.0, 1.0, 0.5], 2), vec![1.0]);
    }

    #[test]
    fn f32_to_s16_clamps_scales_rounds() {
        assert_eq!(f32_to_s16(0.0), 0);
        assert_eq!(f32_to_s16(1.0), 32767);
        assert_eq!(f32_to_s16(2.0), 32767); // clamp high
        assert_eq!(f32_to_s16(-1.0), -32767);
        assert_eq!(f32_to_s16(-2.0), -32767); // clamp low
        assert_eq!(f32_to_s16(0.5), 16384); // 0.5 * 32767 = 16383.5 -> round 16384
    }

    #[test]
    fn resample_identity_when_rates_equal() {
        let v = vec![0.1, 0.2, 0.3];
        assert_eq!(resample_linear(&v, 16_000, 16_000), v);
    }

    #[test]
    fn resample_48k_to_16k_thirds_the_length() {
        let v: Vec<f32> = (0..6).map(|i| i as f32).collect();
        let out = resample_linear(&v, 48_000, 16_000);
        assert_eq!(out.len(), 2); // floor(6 * 16000 / 48000) = 2
        assert_eq!(out[0], 0.0); // first sample preserved
    }

    #[test]
    fn resample_empty_is_empty() {
        assert!(resample_linear(&[], 48_000, 16_000).is_empty());
    }

    #[test]
    fn rms_of_silence_is_zero() {
        assert_eq!(rms(&[0.0; 100]), 0.0);
    }

    #[test]
    fn rms_of_constant_equals_magnitude() {
        assert!((rms(&[0.5; 64]) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn rms_of_full_scale_sine_is_about_root_half() {
        let sine: Vec<f32> = (0..1600)
            .map(|i| (i as f32 * std::f32::consts::TAU / 100.0).sin())
            .collect();
        assert!((rms(&sine) - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01);
    }

    #[test]
    fn peak_is_max_absolute() {
        assert!((peak(&[0.1, -0.9, 0.3]) - 0.9).abs() < 1e-6);
        assert_eq!(peak(&[]), 0.0);
    }

    #[test]
    fn frame_chunker_yields_exact_frames_and_buffers_remainder() {
        let mut c = FrameChunker::new(480);
        let frames = c.push(&vec![0.0; 1000]);
        assert_eq!(frames.len(), 2); // 1000 / 480 = 2 full frames
        assert!(frames.iter().all(|f| f.len() == 480));
        assert_eq!(c.pending(), 40); // 1000 - 960
        // Topping up to the next boundary releases one more frame.
        let more = c.push(&vec![0.0; 440]);
        assert_eq!(more.len(), 1);
        assert_eq!(c.pending(), 0);
    }

    #[test]
    fn frame_samples_is_480() {
        assert_eq!(FRAME_SAMPLES, 480);
    }

    #[test]
    fn device_name_collapses_whitespace() {
        assert_eq!(normalize_device_name("  Mic   (USB)\t Array "), "Mic (USB) Array");
    }
}
