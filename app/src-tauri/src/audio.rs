//! Microphone capture front-end (ADR-001/004, Phase 1) — turns the selected input
//! device into the 16 kHz mono `s16` PCM the warm Whisper model expects, and feeds
//! the HUD level meter. See `docs/specs/audio-capture.md`.
//!
//! The DSP core here is **pure and cargo-tested** (downmix, resample, quantize,
//! RMS/peak, frame chunking, device-name normalization). The real-time cpal stream,
//! the lock-free ring buffer, and the processing/VAD thread (§5) are the runtime
//! seam wired during the orchestrator stage; this file currently also exposes the
//! device-enumeration command the Settings picker needs.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};

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

// ─────────────────────────────────────────────────────────────────────────────
// Live capture (cpal stream on its own thread; runtime-pending verification)
// ─────────────────────────────────────────────────────────────────────────────
//
// WHY a dedicated thread: a cpal `Stream` is `!Send` (WASAPI COM handles), so it
// must be created, driven, and dropped on one thread — it cannot live in managed
// `State`. The thread builds the stream, accumulates mono PCM at the device rate,
// emits `Level` to the HUD, and on stop drops the stream; `end_capture` resamples
// the whole buffer to 16 kHz once (avoiding per-chunk resample artifacts). The
// orchestrator (`dictation.rs`) calls `begin_capture`/`end_capture` in-process.

/// Events streamed to the HUD/orchestrator during capture (spec §2).
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum CaptureEvent {
    Level { rms: f32, peak: f32 },
    Error { message: String },
}

/// A one-shot mic test result for the Hub (spec §2).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicTest {
    pub peak: f32,
    pub rms: f32,
    pub device_name: String,
}

struct CaptureSession {
    stop: Arc<AtomicBool>,
    samples: Arc<Mutex<Vec<f32>>>, // mono, at the device's native rate
    rate: u32,
    handle: Option<JoinHandle<()>>,
}

/// Managed capture state — at most one session at a time (Rule 11).
#[derive(Default)]
pub struct CaptureState {
    session: Mutex<Option<CaptureSession>>,
}

fn resolve_input_device(device_id: Option<&str>) -> Result<cpal::Device, String> {
    let host = cpal::default_host();
    if let Some(id) = device_id {
        if !id.is_empty() && id != "default" {
            let mut devices =
                host.input_devices().map_err(|e| format!("no input device available: {e}"))?;
            return devices
                .find(|d| d.name().map(|n| n == id).unwrap_or(false))
                .ok_or_else(|| "selected device not found".to_string());
        }
    }
    host.default_input_device().ok_or_else(|| "no input device available".to_string())
}

fn push_mono(buf: &Arc<Mutex<Vec<f32>>>, mono: &[f32]) {
    if let Ok(mut b) = buf.lock() {
        b.extend_from_slice(mono);
    }
}

/// Open the cpal stream + accumulate; reports the device sample rate (or an error)
/// back over `ready` once the stream is live, then emits `Level` until stopped.
fn capture_thread(
    device_id: Option<String>,
    stop: Arc<AtomicBool>,
    samples: Arc<Mutex<Vec<f32>>>,
    channel: Option<Channel<CaptureEvent>>,
    app: Option<AppHandle>,
    ready: std::sync::mpsc::Sender<Result<u32, String>>,
) {
    let device = match resolve_input_device(device_id.as_deref()) {
        Ok(d) => d,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let config = match device.default_input_config() {
        Ok(c) => c,
        Err(e) => {
            let _ = ready.send(Err(format!("failed to open audio stream: {e}")));
            return;
        }
    };
    let channels = config.channels();
    let rate = config.sample_rate().0;
    let fmt = config.sample_format();
    let cfg: cpal::StreamConfig = config.into();
    let err_channel = channel.clone();
    let err_fn = move |err: cpal::StreamError| {
        if let Some(ch) = &err_channel {
            let _ = ch.send(CaptureEvent::Error { message: format!("audio stream error: {err}") });
        }
    };

    let cb_buf = samples.clone();
    let stream_res = match fmt {
        SampleFormat::F32 => device.build_input_stream(
            &cfg,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                push_mono(&cb_buf, &downmix_to_mono(data, channels));
            },
            err_fn,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            &cfg,
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                let f: Vec<f32> = data.iter().map(|&s| s as f32 / 32768.0).collect();
                push_mono(&cb_buf, &downmix_to_mono(&f, channels));
            },
            err_fn,
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
            &cfg,
            move |data: &[u16], _: &cpal::InputCallbackInfo| {
                let f: Vec<f32> = data.iter().map(|&s| (s as f32 - 32768.0) / 32768.0).collect();
                push_mono(&cb_buf, &downmix_to_mono(&f, channels));
            },
            err_fn,
            None,
        ),
        other => {
            let _ = ready.send(Err(format!("unsupported sample format: {other:?}")));
            return;
        }
    };
    let stream = match stream_res {
        Ok(s) => s,
        Err(e) => {
            let _ = ready.send(Err(format!("failed to open audio stream: {e}")));
            return;
        }
    };
    if let Err(e) = stream.play() {
        let _ = ready.send(Err(format!("failed to open audio stream: {e}")));
        return;
    }
    let _ = ready.send(Ok(rate));

    // ~50 Hz level meter (Rule 10) until stop; the stream drops when this returns.
    let window = (rate as usize / 50).max(1);
    let mut tick: u32 = 0;
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(20));
        if channel.is_none() && app.is_none() {
            continue;
        }
        // Clone only the trailing window — not the whole (growing) buffer — so the
        // meter stays O(window) per tick regardless of utterance length.
        let recent = match samples.lock() {
            Ok(b) => {
                let start = b.len().saturating_sub(window);
                b[start..].to_vec()
            }
            Err(_) => Vec::new(),
        };
        let level = rms(&recent);
        if let Some(ch) = &channel {
            let _ = ch.send(CaptureEvent::Level { rms: level, peak: peak(&recent) });
        }
        // Drive the floating HUD waveform at ~16 Hz (every 3rd 20 ms tick) — smooth
        // enough without flooding the IPC bridge.
        tick = tick.wrapping_add(1);
        if let (Some(app), 0) = (&app, tick % 3) {
            let _ = app.emit("hud://level", level);
        }
    }
    drop(stream);
}

/// Start capture for one session (in-process; called by the orchestrator). Blocks
/// only until the stream is confirmed live. Rejects a second concurrent session.
pub fn begin_capture(
    state: &CaptureState,
    device_id: Option<&str>,
    channel: Option<Channel<CaptureEvent>>,
    app: Option<AppHandle>,
) -> Result<(), String> {
    let mut guard = state.session.lock().map_err(|_| "capture state poisoned".to_string())?;
    if guard.is_some() {
        return Err("capture already in progress".to_string());
    }
    let stop = Arc::new(AtomicBool::new(false));
    let samples = Arc::new(Mutex::new(Vec::<f32>::new()));
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<u32, String>>();
    let (stop_t, samples_t, device_t) = (stop.clone(), samples.clone(), device_id.map(str::to_string));
    let handle = std::thread::spawn(move || {
        capture_thread(device_t, stop_t, samples_t, channel, app, ready_tx);
    });
    match ready_rx.recv().map_err(|_| "capture thread failed to start".to_string())? {
        Ok(rate) => {
            *guard = Some(CaptureSession { stop, samples, rate, handle: Some(handle) });
            Ok(())
        }
        Err(e) => {
            let _ = handle.join();
            Err(e)
        }
    }
}

/// Stop capture and return the utterance as 16 kHz mono `f32` (resampled once).
pub fn end_capture(state: &CaptureState) -> Result<Vec<f32>, String> {
    let mut guard = state.session.lock().map_err(|_| "capture state poisoned".to_string())?;
    let Some(mut session) = guard.take() else {
        return Ok(Vec::new());
    };
    session.stop.store(true, Ordering::SeqCst);
    if let Some(h) = session.handle.take() {
        let _ = h.join();
    }
    let raw = session.samples.lock().map(|s| s.clone()).unwrap_or_default();
    Ok(resample_linear(&raw, session.rate, SAMPLE_RATE_HZ))
}

fn default_input_name() -> String {
    cpal::default_host()
        .default_input_device()
        .and_then(|d| d.name().ok())
        .map(|n| normalize_device_name(&n))
        .unwrap_or_else(|| "default".to_string())
}

/// One-shot mic test for the Hub: capture briefly, report peak/RMS (no STT, §2).
#[tauri::command]
pub fn test_microphone(state: State<'_, CaptureState>, ms: Option<u32>) -> Result<MicTest, String> {
    let dur = ms.unwrap_or(1500).clamp(200, 5000);
    begin_capture(&state, None, None, None)?;
    std::thread::sleep(Duration::from_millis(dur as u64));
    let samples = end_capture(&state)?;
    Ok(MicTest { peak: peak(&samples), rms: rms(&samples), device_name: default_input_name() })
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
