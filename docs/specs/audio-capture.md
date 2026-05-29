# Audio Capture & VAD Endpointing Feature Spec

> **Status**: Phase 1 — partial: the pure DSP core (`audio.rs`: downmix, linear resample, `f32`→`s16`, RMS/peak, `FrameChunker`, device-name normalize) and the VAD endpoint state machine (`vad.rs`: debounce/hangover, Rules 4/5/8) are implemented & cargo-tested, and `list_input_devices` is live. The cpal capture path is implemented (compile/build-verified, **runtime-validated on Windows**): a `!Send`-safe capture thread builds the stream, accumulates mono PCM, emits `Level`, and `end_capture` resamples to 16 kHz once; `begin_capture`/`end_capture` (in-process for the orchestrator) + the `test_microphone` command + Hub mic-test button. **A session never auto-ends on silence** — it ends only on an explicit user action (hotkey release / 2nd toggle press); a pause mid-utterance keeps recording. The earlier energy-gated auto-endpoint was **removed** (it cut dictations on natural pauses). Anti-hallucination VAD-gated trimming is still applied at transcription time by whisper-server (`--vad`). The `vad.rs` `EndpointDetector` is retained, cargo-tested, as the primitive for the backlog **in-process per-frame Silero** endpoint path — not wired to the live path today.
> **Last updated**: 2026-05-29
> **Coverage**: all sections drafted (1–9)
> **Environment**: desktop (Windows, native)

This spec owns the **front of the dictation pipeline**: turning the user's microphone into the
16 kHz mono PCM stream the warm Whisper model expects, and using **Silero VAD** to decide *where
speech is* — trimming leading/trailing silence and (in toggle mode) detecting when an utterance
has ended. It sits between the hotkey and the STT in the chain
**hotkey → _capture → VAD_ → STT → cleanup → inject**: the orchestrator in
[dictation.md](dictation.md) starts/stops capture on the hotkey, this module produces clean speech
frames, and [speech-to-text.md](speech-to-text.md) consumes them. It also feeds the **RMS level
meter** that drives the floating mic HUD waveform ([tray-and-hud.md](tray-and-hud.md)). It lands
in **Phase 1 — Core Dictation MVP** ([../ROADMAP.md](../ROADMAP.md)) and implements
[ADR-001](architecture.md#adr-001-native-on-device-privacy-first) (audio stays in memory, never
disk, never the network), [ADR-004](architecture.md#adr-004-warmresident-stt-for-live-dictation)
(frames feed a *warm* model — no per-utterance cold spawn), and
[ADR-007](architecture.md#adr-007-on-demand-model-download--cpu-bundled--optional-cuda-engine)
(Silero VAD is part of the fixed anti-hallucination defaults; the VAD model is downloaded
on-demand like the Whisper weights).

**Scope decisions** (locked at design time):

- **`cpal` for capture (WASAPI on Windows)** — one cross-Windows mic stack with a real-time
  callback; we do **not** shell out to ffmpeg for *live* capture (ffmpeg is Toolzy's *file*-mode
  preprocessor and is reused only by the backlog file-transcription mode, not here)
  ([ADR-004](architecture.md#adr-004-warmresident-stt-for-live-dictation) / Phase 1).
- **Fixed capture format: 16 kHz, mono, PCM `f32` → `s16`** — Whisper's native input rate. We
  resample/downmix in-process at capture time so the warm model never has to; the device may run
  at 44.1/48 kHz and we convert (Phase 1).
- **Silero VAD for anti-hallucination silence gating** — the same Silero model Toolzy already
  ships, applied server-side by whisper-server (`--vad`) so silence/non-speech can't become
  hallucinated text. RMS energy is used **only** for the HUD meter, never to decide speech
  boundaries; and **silence never ends a session** — only an explicit user action does (Rule 5)
  ([ADR-007](architecture.md#adr-007-on-demand-model-download--cpu-bundled--optional-cuda-engine) /
  Phase 1).
- **Audio never touches disk** — frames live in an in-memory ring buffer and are dropped after the
  utterance; no WAV is written on the live path
  ([ADR-001](architecture.md#adr-001-native-on-device-privacy-first)).
- **Capture lives entirely in Rust** — `audio.rs` + `vad.rs` are engine modules; the Svelte UI
  only picks a device and renders the level meter via typed `invoke()` wrappers (Phase 0/1).
- **No always-on listening in v1** — capture starts on a hotkey and stops on release/endpoint;
  there is no wake word and no background recording (wake word is Phase 5 / Backlog).

---

## 1. Inputs / Outputs

| Aspect | This feature |
|---|---|
| **Trigger** | dictation start from the orchestrator (global push-to-talk hold or toggle; see [hotkeys.md](hotkeys.md) / [dictation.md](dictation.md)) |
| **Audio in** | the selected input device via **cpal/WASAPI** — whatever native rate/format the device offers (e.g. 48 kHz stereo `f32`) |
| **Text in** | N/A |
| **Text out** | N/A — this module emits **audio frames**, not text |
| **Primary output** | a stream of **16 kHz mono `s16` PCM** frames of *detected speech only* (leading/trailing silence trimmed), handed to the warm STT ([speech-to-text.md](speech-to-text.md)) |
| **Side output** | a low-rate **RMS level** signal (0.0–1.0) streamed to the HUD waveform meter ([tray-and-hud.md](tray-and-hud.md)) and **VAD state events** (speech-started / speech-ended) to the orchestrator |
| **Target** | the warm STT worker (in-memory PCM handoff); the HUD (level meter); the Hub settings (device list) |
| **Language** | language-agnostic — VAD and capture are independent of spoken language (pt-BR / English / any) |

Engines/crates behind each path: **`cpal`** (capture + device enumeration, WASAPI backend),
an in-process **resampler/downmixer** (linear or `rubato`-style resample 48 kHz→16 kHz, average
channels to mono), and **Silero VAD** (the same `ggml-silero-v6.2.0.bin` model Toolzy downloads,
run in-process). The audio buffer **never touches disk** and **never leaves the machine**
([ADR-001](architecture.md#adr-001-native-on-device-privacy-first)); the only network use anywhere
near this module is the one-time on-demand download of the small VAD model from Hugging Face,
done Rust-side alongside the Whisper weights
([ADR-007](architecture.md#adr-007-on-demand-model-download--cpu-bundled--optional-cuda-engine),
[onboarding.md](onboarding.md)). Latency cap: capture-side conversion + VAD must stay well under
the STT inference cost so this stage is effectively free on the hot path.

---

## 2. Engine Contract (Rust)

Rust is the **engine**; the Svelte UI is a thin webview that calls typed `invoke()` wrappers (see
[architecture.md](architecture.md)). All commands return `Result<T, String>` — no panics across
the IPC boundary ([ADR-006](architecture.md#adr-006-resultt-string-error-model-across-the-rust--ui-ipc)).

**Modules**: `app/src-tauri/src/audio.rs` (device enumeration, cpal stream, ring buffer,
resample/downmix, RMS) · `app/src-tauri/src/vad.rs` (Silero load + frame classification +
endpoint state machine).

There are **two surfaces** here. (a) The **registered Tauri commands** the Svelte UI calls
directly (device list + mic test + the privacy deep-link). (b) The **in-process orchestrator
helpers** (`begin_capture` / `end_capture`) that the dictation orchestrator
([dictation.md](dictation.md)) calls on the Rust side on hotkey press/release — these are plain
`pub fn`, **not** `#[tauri::command]`s, because live capture is driven entirely Rust-side (the UI
never starts/stops a dictation session; it only presses the hotkey). **Device selection is not its
own command** — it is persisted as `settings.audio.input_device` through `update_settings`
([settings.md](settings.md)), and the orchestrator passes that id into `begin_capture`.

```rust
// ── audio.rs — REGISTERED COMMANDS (UI-callable) ──────────────────────────────
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AudioDevice { id: String, name: String, is_default: bool }

/// Enumerate input devices for the Settings picker. Pure-ish (queries cpal hosts).
#[tauri::command]
fn list_input_devices() -> Result<Vec<AudioDevice>, String>;

/// One-shot mic test for onboarding/settings: capture briefly from the default mic and
/// return peak/RMS so the UI can show "we can hear you" without running STT. The `level`
/// Channel streams `CaptureEvent::Level` for a live meter while the test runs (required —
/// Tauri accepts only a bare `Channel<T>`, not `Option<Channel<T>>`; callers that don't
/// want the meter simply ignore it).
#[tauri::command]
fn test_microphone(
    state: State<'_, CaptureState>,
    ms: Option<u32>,
    level: tauri::ipc::Channel<CaptureEvent>,
) -> Result<MicTest, String>;

/// Deep-link the user to Windows' microphone privacy settings (used by the denied-permission
/// error affordance in the Hub/HUD — see §6/§7).
#[tauri::command]
fn open_mic_privacy() -> Result<(), String>;

// ── audio.rs — IN-PROCESS ORCHESTRATOR HELPERS (NOT commands) ─────────────────
// Called by dictation.rs on hotkey press/release; the UI never invokes these. The
// `device_id` comes from `settings.audio.input_device` (resolved by the orchestrator,
// `"default"`/None = OS default — Rule 14). The optional Channel streams level events to
// the HUD; speech samples are returned by value (in-process), never over the Channel.

/// Open the cpal stream and start capture for one dictation session. Rejects a second
/// concurrent session (Rule 11). Blocks only until the stream is confirmed live.
pub fn begin_capture(
    state: &CaptureState,
    device_id: Option<&str>,
    channel: Option<tauri::ipc::Channel<CaptureEvent>>,
    app: Option<tauri::AppHandle>,
) -> Result<(), String>;

/// Stop capture (hotkey release / toggle-off / cancel), tear down the cpal stream, and
/// return the utterance as 16 kHz mono `f32` (resampled once). No active session → `Ok(vec![])`.
pub fn end_capture(state: &CaptureState) -> Result<Vec<f32>, String>;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
enum CaptureEvent {
    Level { rms: f32, peak: f32 },          // ~30–60 Hz, for the HUD waveform meter
    SpeechStart,                            // VAD crossed into speech
    SpeechEnd,                              // VAD endpoint reached (utterance boundary)
    Error { message: String },              // device lost mid-stream, etc.
}
```

> **Not implemented (Phase-pending).** A UI-facing capture surface — discrete
> `set_input_device` / `start_capture` / `stop_capture` **commands** — was envisioned in early
> drafts but is **not built and not registered**. In the shipped MVP the UI does not start/stop
> dictation (the global hotkey does, Rust-side, via `begin_capture`/`end_capture`), and device
> choice is persisted through `update_settings` (`settings.audio.input_device`), so no
> `set_input_device` command exists. These would only land if a future phase adds a UI-driven
> capture surface (e.g. an in-window "record" button distinct from the global PTT hotkey).

```rust
// ── vad.rs (pure-ish: load model once, classify fixed-size frames) ─────────────
struct Vad { /* Silero session + ring of recent frame probabilities */ }
impl Vad {
    fn load(model_path: &Path) -> Result<Self, String>;     // ggml-silero-v6.2.0.bin
    fn push_frame(&mut self, frame_16k_mono: &[i16]) -> VadDecision; // 30 ms frames
    fn reset(&mut self);
}
enum VadDecision { Silence, SpeechOngoing, SpeechStarted, SpeechEnded }
```

- **Capture mode** (`pushToTalk` | `toggle`, default `pushToTalk`) and `vadEnabled` (default
  `true`; off only for diagnostics) are orchestrator-side concerns ([dictation.md](dictation.md)),
  not arguments the UI passes here. The device id `begin_capture` receives comes from
  `settings.audio.input_device` (`"default"`/None → OS default). VAD endpointing parameters
  (thresholds/hangover) are **fixed defaults** — see §4.
- `MicTest` (`camelCase`): `{ peak: f32, rms: f32, deviceName: String }`.
- `Err(String)` cases (each maps 1:1 to a UI error state, §6/§7): `"no input device available"`,
  `"microphone access denied — enable it in Windows Settings → Privacy → Microphone"`,
  `"input device is in use by another application"`, `"failed to open audio stream: …"`,
  `"VAD model not downloaded"`, `"capture already in progress"`, `"selected device not found"`.
  These surface from `begin_capture`/`end_capture` (live path) and `test_microphone` (mic test).
- Native in-process only — `cpal` (WASAPI) and the Silero model run inside MIA's process. **No
  sidecar, no ffmpeg, no `whisper-cli` cold spawn** on this path
  ([ADR-004](architecture.md#adr-004-warmresident-stt-for-live-dictation)).
- **Pure helpers** behind `#[cfg(test)]` cargo tests (no I/O): the resampler (48 kHz→16 kHz
  ratio + filtering on a fixed input buffer), the stereo→mono downmix, `f32`→`s16` quantization
  (clamp + round), RMS/peak computation, the **VAD endpoint state machine** (`VadDecision`
  transitions given a synthetic sequence of frame probabilities), and frame chunking (splitting
  arbitrary callback buffers into fixed 30 ms frames). The device-enumeration name normalizer is
  also pure.
- Typed UI wrapper: `app/src/lib/audio.ts` exposes only the registered commands —
  `listInputDevices()` (`invoke<AudioDevice[]>("list_input_devices")`), `testMicrophone()`, and
  `openMicPrivacy()`. There is **no** `setInputDevice`/`startCapture`/`stopCapture` wrapper: device
  choice is saved through the settings wrapper ([settings.md](settings.md)), and live capture is
  driven by the global hotkey on the Rust side. The UI holds **no** capture or VAD logic — it
  renders the device picker and the level meter only.

---

## 3. Business Rules

1. **Fixed output format** — regardless of the device's native rate/channels, the frames handed to
   STT are **16 kHz, mono, signed-16-bit PCM**. A 48 kHz stereo `f32` device is downmixed to mono
   and resampled to 16 kHz in-process; the `f32`→`s16` conversion clamps to `[-1.0, 1.0]` then
   scales to `i16::MIN..=i16::MAX`. (cargo test: downmix + resample + quantize on fixed buffers.)
2. **Capture starts only on an explicit trigger** — the cpal stream is opened on `start_capture`
   (driven by the hotkey) and closed on `stop_capture`. There is **no** background/always-on
   recording in v1 (Rule traces to scope: no wake word).
3. **Silero VAD gates what reaches STT** — only frames classified as speech (plus a small
   pre/post pad, Rule 6) are forwarded to the warm model. Pure silence is dropped, so Whisper
   never sees silence-only audio that it would hallucinate over
   ([ADR-007](architecture.md#adr-007-on-demand-model-download--cpu-bundled--optional-cuda-engine)).
4. **Leading silence is trimmed** — audio captured before `SpeechStart` (minus the pre-roll pad,
   Rule 6) is discarded, not transcribed. (cargo test: state machine emits `SpeechStarted` only
   after `MIN_SPEECH_MS` of speech frames.)
5. **A session ends only on an explicit user action — never on silence.** In `pushToTalk` the
   utterance ends on **hotkey release**; in `toggle` it ends on the **second press**. A run of
   silence, however long, does **not** end a session (the user may pause to think mid-utterance).
   The `vad.rs` `EndpointDetector` (which would emit `SpeechEnded` after `MIN_SILENCE_MS`) is a
   cargo-tested primitive for the **backlog** in-process Silero endpoint path; it is not wired to
   the live path. (The energy-gated auto-endpoint that previously cut toggle sessions on a pause
   was removed.)
6. **Pre/post-roll padding** — keep a small **pre-roll** (≈ `PRE_ROLL_MS`) of audio *before*
   `SpeechStart` and a small **post-roll** *after* the last speech frame, so word onsets/codas
   are not clipped. The ring buffer must retain at least `PRE_ROLL_MS` of history at all times.
7. **Trailing silence is trimmed** — on stop (release or endpoint), silence beyond the post-roll
   pad is not sent to STT.
8. **All-silence utterance yields nothing** — if a session ends with no `SpeechStarted` ever
   emitted, `stop_capture` returns `Ok` but **no frames are sent to STT** and the orchestrator
   injects nothing (returns to Idle, no empty/hallucinated text). (cargo test: state machine over
   an all-silence probability sequence never emits `SpeechStarted`.)
9. **VAD model required** — if `ggml-silero-v6.2.0.bin` is not present, `start_capture` returns
   `Err("VAD model not downloaded")`; the orchestrator routes the user to the download gate
   ([onboarding.md](onboarding.md), [speech-to-text.md](speech-to-text.md)). VAD cannot be
   silently skipped on the live path (it's a fixed anti-hallucination default).
10. **Level meter is energy-based and decoupled from VAD** — `Level` events carry RMS/peak
    computed directly from the PCM and are used **only** for the HUD waveform; they never decide
    speech boundaries (that is Silero's job, Rule 3). RMS is reported on the `[0.0, 1.0]` scale.
    (cargo test: RMS of a known sine/silence buffer.)
11. **Single capture at a time** — if `start_capture` is called while a session is active, it
    returns `Err("capture already in progress")`; it does not open a second stream.
12. **Device loss mid-stream is surfaced, not swallowed** — if the device is unplugged/disabled
    during capture, emit `CaptureEvent::Error` and stop cleanly; the orchestrator shows the HUD
    error and returns to Idle (no partial garbage injected).
13. **Audio never persists** — frames live only in the in-memory ring buffer for the current
    utterance and are dropped afterward; nothing is written to disk and nothing is sent over the
    network ([ADR-001](architecture.md#adr-001-native-on-device-privacy-first)). (Verified by
    audit/manual; no file I/O in the capture path.)
14. **Default device follows the OS** — with no explicit selection, capture uses the current OS
    default input device, re-resolved at each `start_capture` (so changing the Windows default
    takes effect on the next dictation without restarting MIA).

---

## 4. Options & Defaults

| Option | Type | Range / values | Default | Effect |
|---|---|---|---|---|
| `inputDeviceId` | `Option<string>` | any id from `list_input_devices` | `None` (OS default) | which mic to capture (Settings picker) |
| `mode` | enum | `pushToTalk` \| `toggle` | `pushToTalk` | release-ends vs second-press-ends an utterance (Rule 5; see [hotkeys.md](hotkeys.md)) |
| `vadEnabled` | bool | `true` \| `false` | `true` | gate STT by speech (Rule 3). `false` is **diagnostics-only**, not exposed as a normal setting |
| Capture sample rate | fixed | 16000 Hz | **16000** | Whisper's native rate; not user-tunable |
| Capture channels | fixed | mono | **mono** | downmixed from device channels; not user-tunable |
| Sample format to STT | fixed | `s16` | **`s16`** | from device `f32`; not user-tunable |
| `FRAME_MS` | fixed | 30 ms | **30** | VAD/processing frame size (Silero operates on small fixed frames) |
| `PRE_ROLL_MS` | fixed | ~200 ms | **200** | audio retained before `SpeechStart` (Rule 6) |
| `POST_ROLL_MS` | fixed | ~200 ms | **200** | audio retained after last speech frame (Rule 6) |
| `MIN_SPEECH_MS` | fixed | ~150 ms | **150** | min run of speech frames before `SpeechStarted` (debounces blips, Rule 4) |
| `MIN_SILENCE_MS` | fixed | ~700 ms | **700** | silence run that triggers `SpeechEnded` in the `EndpointDetector` primitive — **not wired live** (backlog Silero endpoint path, Rule 5) |
| `VAD_THRESHOLD` | fixed | 0.0–1.0 | **~0.5** | Silero speech-probability threshold |
| `LEVEL_HZ` | fixed | ~30–60 Hz | **~50** | how often `Level` events are emitted for the HUD |

The VAD/endpoint constants are **fixed defaults** (part of the anti-hallucination contract,
[ADR-007](architecture.md#adr-007-on-demand-model-download--cpu-bundled--optional-cuda-engine)),
not user-tunable knobs in v1 — mirroring the locked Whisper decoding policy (greedy temperature 0 +
`temperature_inc=0.0` to disable the fallback ladder + stateless per-`/inference` calls for no
cross-utterance context — the equivalents of whisper-CLI's `--no-fallback` / `--max-context 0`, not
those literal flags). The Settings UI validates only the device
choice; the engine re-resolves the device defensively at `start_capture` (Rule 14) and re-checks
the VAD model exists (Rule 9). The Silero model file and its source are reused verbatim from
Toolzy: `ggml-silero-v6.2.0.bin` from
`https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin`.

---

## 5. Threading / Performance

- **Audio thread (cpal callback)** — capture runs on **cpal's real-time audio callback thread**.
  Inside the callback we do the **minimum**: copy the incoming buffer, downmix to mono, and push
  into a **lock-free SPSC ring buffer** (e.g. an `rtrb`/`ringbuf`-style producer). **No** model
  inference, **no** allocation storms, **no** locks, **no** logging-to-disk happen in the
  callback ([architecture.md → Threading](architecture.md#threading-audio-thread-vs-command-execution)).
- **Processing/VAD thread** — a dedicated worker drains the ring buffer, resamples 48 kHz→16 kHz,
  quantizes `f32`→`s16`, chunks into 30 ms frames, runs **Silero VAD** per frame, computes RMS
  for the level meter, and forwards *speech* frames to the warm STT worker. This thread (not the
  audio callback) owns the endpoint state machine and emits `CaptureEvent`s. Keeping VAD off the
  audio thread protects the real-time callback; keeping it off the STT worker keeps capture
  flowing while inference runs.
- **Lock-free handoff** — audio callback → processing thread is **SPSC ring buffer**; processing
  thread → warm STT is an in-process channel of `s16` frames (no WAV, no IPC, no HTTP hop on the
  default in-process backend). This is the concrete realization of the warm-model handoff in
  [ADR-004](architecture.md#adr-004-warmresident-stt-for-live-dictation): the model is loaded
  **once** and fed live frames; **this module never cold-spawns `whisper-cli`**.
- **Latency budget** — capture-side work (downmix + resample + `f32`→`s16` + RMS + Silero per
  30 ms frame) must be a small fraction of one frame's wall-clock, so it adds negligibly to the
  end-to-end *utterance-end → first injected char* budget. The dominant cost downstream is STT
  inference ([speech-to-text.md](speech-to-text.md)), not capture. Silero inference per 30 ms
  frame is cheap on CPU and runs comfortably real-time.
- **Cancellation** — `stop_capture` (hotkey release, toggle-off, or an abort/timeout from the
  orchestrator) sets a cancel flag in managed `State`, stops the cpal stream, and the processing
  thread flushes per mode (push-to-talk: send buffered speech up to release; cancel/abort: discard
  in-flight frames so **no stale text** is ever injected). The ring buffer is cleared on stop.
- **Resource use** — the Silero VAD model is **tiny** (≈ a couple MB) and loaded once into the
  processing thread; it adds negligible RAM next to the warm Whisper model
  ([ADR-004](architecture.md#adr-004-warmresident-stt-for-live-dictation)). The ring buffer is a
  small fixed allocation (a few seconds of 16 kHz mono `s16` ≈ tens to low-hundreds of KB) — audio
  stays bounded and in memory only (Rule 13).

---

## 6. UI States

This module drives the **HUD** through its `idle → listening → transcribing` portion (it owns
*listening*; STT owns *transcribing*, injection owns *inserting*) and surfaces a **device picker**
+ **mic test** in the Settings/Hub window. See [tray-and-hud.md](tray-and-hud.md) and
[design-system.md](design-system.md).

```
States: Idle(hidden) → Listening(pulsing waveform, RMS-driven) → Transcribing(spinner) → …
Transitions:
  start_capture            → Listening   (HUD appears; waveform animates off Level events)
  VAD SpeechStart          → Listening   (waveform "active" accent)
  hotkey release           → hand off to STT → Transcribing   (push-to-talk)
  2nd toggle press         → hand off to STT → Transcribing   (toggle; silence never ends it)
  all-silence + stop       → Idle         (no text; Rule 8)
  device lost / denied     → Error(message) → Idle
```

- **HUD (while listening)** — the dark, translucent, always-on-top mic pill. The **waveform /
  level meter is driven by the `Level` (RMS/peak) events** from §2, animating with the
  **action-blue "listening" pulse** (the single accent color). When VAD reports `SpeechEnd`/the
  hotkey releases, the HUD transitions to the transcribing spinner (owned by
  [speech-to-text.md](speech-to-text.md)). The HUD transitions only when the user ends the
  session (release / 2nd press) — never on a silent pause. Errors (no mic, denied, device lost)
  render the HUD **error** state with a short message. Keep click-through where possible.
- **Settings/Hub (light theme)** — an **input-device picker** (`list_input_devices` →
  `set_input_device`; "System default" first), and a **"Test microphone"** button
  (`test_microphone`) that shows a live level bar / "we can hear you" confirmation without running
  STT. If permission is denied, show an inline explainer + a button/link to **Windows Settings →
  Privacy & security → Microphone** (Rule/Edge: denied permission).
- Respect the one-action-color discipline, ≥40px hit targets, and don't rely on color alone — the
  level meter also animates (motion), and states carry text/icon, not just hue.

---

## 7. Edge Cases

| Scenario | Expected behavior |
|---|---|
| No microphone present | `Err("no input device available")`; HUD error; onboarding/settings prompt to connect a mic |
| Microphone permission denied (Windows privacy) | `Err("microphone access denied — enable it in Windows Settings → Privacy → Microphone")`; HUD/Hub explainer with a deep link; no capture |
| Mic exclusively held by another app | `Err("input device is in use by another application")`; HUD error; suggest closing the other app or picking another device |
| Selected device unplugged before start | `Err("selected device not found")`; fall back to OS default on next attempt; surface in Hub |
| Device lost **mid-stream** | emit `CaptureEvent::Error`, stop cleanly, return to Idle; never inject partial/garbage text (Rule 12) |
| All-silence utterance (VAD never fires) | no frames to STT, no injection, return to Idle — no empty or hallucinated text (Rule 8) |
| Very short blip (cough/click) | debounced by `MIN_SPEECH_MS`; does not count as speech start (Rule 4) |
| VAD model not downloaded | `Err("VAD model not downloaded")`; route to download gate ([onboarding.md](onboarding.md)) (Rule 9) |
| Device runs at 44.1/48 kHz, stereo | in-process downmix + resample to 16 kHz mono `s16` (Rule 1) |
| `start_capture` while already capturing | `Err("capture already in progress")`; no second stream (Rule 11) |
| Long pause mid-utterance (toggle mode) | keeps recording — silence never ends a session; only the 2nd press does (Rule 5) |
| Hotkey released before VAD `SpeechStart` (push-to-talk) | treat as all-silence: nothing sent to STT (Rule 8) |

---

## 8. Testing Checklist

- **Rust** (`cargo test`, no I/O — pure helpers only):
  - [x] stereo→mono downmix on fixed buffers (averaging, clipping behavior)
  - [x] 48 kHz→16 kHz resample ratio + output length on a fixed input buffer
  - [x] `f32`→`s16` quantization: clamp to `[-1.0,1.0]`, scale, round, no overflow
  - [x] RMS/peak on known buffers (silence → ~0; full-scale sine → expected RMS) (Rule 10)
  - [x] frame chunking: arbitrary callback buffer → exact 30 ms frames + remainder handling
  - [x] **VAD endpoint state machine**: synthetic probability sequences →
        `SpeechStarted` only after `MIN_SPEECH_MS`; `SpeechEnded` only after `MIN_SILENCE_MS`;
        all-silence never emits `SpeechStarted` (Rules 4, 5, 8)
  - [ ] each `Err(String)` path the commands return (no device / denied / in-use / VAD missing /
        already-capturing / device-not-found) — pending the capture commands
- **Manual / runtime** (needs a real mic):
  - [ ] device picker lists mics; selecting one switches capture; "System default" follows OS
  - [ ] `test_microphone` shows a live level without running STT
  - [ ] happy path: hold hotkey, speak (pt-BR and English), release → speech frames reach STT and
        text appears at the cursor ([dictation.md](dictation.md))
  - [ ] HUD waveform animates with voice level and idles to flat on silence
  - [ ] toggle mode keeps recording through a natural pause; only the 2nd press ends it
  - [ ] leading/trailing silence trimmed (no clipped word onsets thanks to pre/post-roll)
  - [ ] all-silence press injects nothing; HUD returns to Idle
  - [ ] deny mic permission in Windows → clear error + deep link; re-grant → works
  - [ ] unplug the mic mid-dictation → clean error, no garbage injected
  - [ ] confirm (audit) no audio file is written and no network call carries audio (Rule 13)

---

## 9. Out of Scope (this version)

- **Always-on listening / "Hey MIA" wake word** — v1 captures only on a hotkey; continuous
  listening + wake word is Phase 5 / Backlog ([../ROADMAP.md](../ROADMAP.md)).
- **Streaming live partials** — v1 endpoints then transcribes; incremental partial results while
  speaking are Phase 5 / Backlog (would re-use this frame stream feeding a streaming STT).
- **Whisper Mode (quiet/whispered speech) tuning** — Phase 5 / Backlog.
- **Noise suppression / AEC / beamforming** — no DSP cleanup beyond downmix+resample in v1; we
  rely on Silero VAD + Whisper robustness.
- **User-tunable VAD thresholds** — fixed defaults in v1 (anti-hallucination contract,
  [ADR-007](architecture.md#adr-007-on-demand-model-download--cpu-bundled--optional-cuda-engine));
  may be exposed later if needed.
- **File / system-audio / loopback capture** — live mic only. File-transcription mode (Toolzy's
  ffmpeg path) is a separate backlog item ([architecture.md → Future](architecture.md#future-file-transcription-mode-reusing-toolzys-ffmpeg-path), [../ROADMAP.md](../ROADMAP.md) Phase 5).
- **macOS / Linux capture backends** — Windows/WASAPI only for v1
  ([ADR-011](architecture.md#adr-011-windows-only-v1-deliberate)); cpal keeps the door open.
