# Dictation Feature Spec

> **Status**: Phase 1 — pure orchestrator core implemented & cargo-tested in `dictation.rs`: the `next_phase` HUD state machine (Idle/Listening/Transcribing/Inserting/Error, illegal-signal no-op, cancel-from-any), `interpret_down` (trigger-mode), `classify_cancel`, `build_result` (latency), and the `Phase`/`DictationEvent`/`DictationResult` types. The `start/stop/cancel_dictation` commands wire the **real pipeline** end-to-end — cpal capture → warm whisper-server STT → deterministic cleanup → dictionary → snippets → injection, emitting HUD `DictationEvent`s + recording stats (compile/build-verified; runtime-validated on Windows). `dictation.ts` wrapper. push-to-hold MVP: start opens capture, stop runs the tail. A session ends only on an explicit user action (release / 2nd toggle press) — it never auto-ends on a pause in speech. Runtime-pending: the `tauri-plugin-global-shortcut` trigger wiring (separate — see [hotkeys.md](hotkeys.md)) and the live HUD waveform `Level` forwarding.
> **Last updated**: 2026-05-29
> **Coverage**: Sections 1-9 drafted. The `start/stop/cancel_dictation` commands exist and are registered in `lib.rs`.
> **Environment**: desktop (Windows, native)

This is MIA's core feature — the end-to-end **dictation orchestration** that wires every other
module together. The user holds (or toggles) a global push-to-talk hotkey, a floating mic HUD
appears, the mic is captured, speech is endpointed, transcribed by a warm Whisper model, cleaned
up, and the resulting text is typed at the cursor in whatever app is focused. The audio never
leaves the machine ([ADR-001](architecture.md#adr-001-native-on-device-privacy-first)). This doc
specifies the **conductor**: the `dictation.rs` module and its state machine that sequence
[hotkeys.md](hotkeys.md) → [audio-capture.md](audio-capture.md) → [speech-to-text.md](speech-to-text.md)
→ [text-cleanup.md](text-cleanup.md) → [text-injection.md](text-injection.md), and the events it
streams to the HUD ([tray-and-hud.md](tray-and-hud.md)). It lands in **Phase 1 — Core Dictation
MVP** ([../ROADMAP.md](../ROADMAP.md)) and implements ADR-001, **ADR-004 (warm STT)**,
ADR-005, ADR-006. The sub-modules own the *how* of each stage; this doc owns the *sequencing,
timing, cancellation, and failure flow* across them.

**Scope decisions** (locked at design time):

- **One pipeline, two trigger modes only** — *push-to-hold* (record while held, endpoint on
  release) and *press-to-toggle* (press to start, press again or VAD-silence to stop). No always-on
  listening, no wake word in v1 (deferred to Phase 5 — [../ROADMAP.md](../ROADMAP.md)). Trigger
  mechanics live in [hotkeys.md](hotkeys.md); this doc consumes the start/stop signals.
- **Single active session** — only one dictation session runs at a time. A second hotkey-down while
  a session is active is interpreted by the active mode (toggle-stop or ignored), never a second
  concurrent capture. Re-entry is rejected at the engine boundary (Rule 12).
- **Warm model, not cold spawn** — the STT model is resident in the running `whisper-server`
  sidecar across utterances
  ([ADR-004](architecture.md#adr-004-warmresident-stt-for-live-dictation)); `dictation.rs`
  **never** cold-spawns a CLI per utterance. This is the latency-critical divergence from
  Toolzy's file mode ([REUSE-FROM-TOOLZY.md](../REUSE-FROM-TOOLZY.md)).
- **Faithful, not creative, by default** — the always-on path is STT → deterministic cleanup →
  inject. No LLM is on the hot path; AI Command Mode / Polish ([ai-commands.md](ai-commands.md)) is
  Phase 2 and opt-in ([ADR-008](architecture.md#adr-008-hybrid-text-intelligence)).
- **Audio stays in RAM** — captured PCM is held in memory and discarded after transcription; it is
  never written to disk ([ADR-001](architecture.md#adr-001-native-on-device-privacy-first)).
- **Inject at the live cursor; no target picker** — text goes to whatever window currently has
  focus when injection fires ([ADR-005](architecture.md#adr-005-system-wide-text-injection-on-windows)).
  Choosing a window is out of scope.

---

## 1. Inputs / Outputs

| Aspect | This feature |
|---|---|
| **Trigger** | global push-to-talk hotkey (hold or toggle) via `tauri-plugin-global-shortcut`; also a tray "Start dictation" action and an Escape/abort signal — see [hotkeys.md](hotkeys.md), [tray-and-hud.md](tray-and-hud.md) |
| **Audio in** | `cpal` 16 kHz mono PCM `f32` stream, handed off the real-time callback via a ring buffer — see [audio-capture.md](audio-capture.md) |
| **Text in** | raw transcript string from the warm STT engine — see [speech-to-text.md](speech-to-text.md) |
| **Text out** | cleaned UTF-8 string injected at the cursor via `enigo` SendInput (clipboard+Ctrl+V fallback for long text) — see [text-cleanup.md](text-cleanup.md), [text-injection.md](text-injection.md) |
| **Target** | the OS-focused window (live cursor); plus the floating mic **HUD** as the status surface |
| **Language** | pt-BR and English first-class; auto-detect or pinned per [speech-to-text.md](speech-to-text.md) |

Engine/crate per stage: `tauri-plugin-global-shortcut` (trigger) → `cpal` + Silero VAD (capture/endpoint) →
warm `whisper-server` sidecar (STT) → pure Rust cleanup module
(format) → `enigo` + `arboard` (inject). **The audio buffer never touches disk** — it lives only in
the session ring buffer / accumulation `Vec<f32>` and is dropped when the session ends
([ADR-001](architecture.md#adr-001-native-on-device-privacy-first)).

---

## 2. Engine Contract (Rust)

Rust is the engine; the Svelte UI is a thin webview that calls one typed `invoke()` wrapper group
and renders HUD state from a `Channel` ([architecture.md](architecture.md)). All commands return
`Result<T, String>` — no panics across the IPC boundary
([ADR-006](architecture.md#adr-006-resultt-string-error-model-across-the-rust--ui-ipc)).

**Module**: `app/src-tauri/src/dictation.rs` (the orchestrator). It coordinates
`audio.rs`, `vad.rs`, `stt.rs`, `cleanup.rs`, `inject.rs`, and reads start/stop from `hotkey.rs`.

```rust
// CONCEPTUAL view of the long-lived state. In the code this is NOT one unified
// `DictationState` managed type — it is split across several Tauri-managed States
// (CaptureState, SttState, SettingsState, DictState, SnippetState, StatsState, …),
// each injected independently into the commands below.
struct DictationState {        // conceptual; not a real managed type
    phase: Mutex<Phase>,            // Idle | Listening | Transcribing | Inserting | Error
    session: Mutex<Option<Session>>,// active audio stream + accumulation buffer + start instant
    cancel: Arc<AtomicBool>,        // set by abort/escape; checked between stages
    engine: Mutex<SttEngine>,       // the WARM whisper-server handle (ADR-004), started once
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
enum DictationEvent {
    StateChanged { phase: Phase },          // drives the HUD state machine
    Level { rms: f32 },                     // live mic level for the HUD waveform
    Injected { chars: usize, ms: u64 },     // success summary (chars typed, end-to-end ms)
    Cancelled { reason: CancelReason },     // user-escape | empty-speech | timeout
    Error { message: String },              // maps 1:1 to a HUD error state
}

/// Begin a session: opens cpal capture into the session buffer and emits the
/// `Listening` StateChanged. `events` streams StateChanged/Level/Injected/Error to
/// the HUD. The transcript/`DictationResult` is produced by `stop_dictation` (the
/// tail of the pipeline), not here — `start_dictation` only opens the capture.
#[tauri::command]
fn start_dictation(
    app: AppHandle,
    capture: State<'_, CaptureState>,
    settings: State<'_, SettingsState>,
    focus: State<'_, FocusContext>,   // captures the focused-app EXE for the per-app style
    events: tauri::ipc::Channel<DictationEvent>,
) -> Result<(), String>;

/// End-of-speech signal for push-to-hold (hotkey released). Stops capture and runs
/// the tail end-to-end: endpoint → warm STT → cleanup → dictionary → snippets →
/// inject, emitting HUD events and recording stats; returns the session summary.
/// State is split across several managed States, each injected independently.
#[tauri::command]
fn stop_dictation(
    app: AppHandle,
    capture: State<'_, CaptureState>,
    stt_state: State<'_, SttState>,
    settings: State<'_, SettingsState>,
    dict: State<'_, DictState>,
    snips: State<'_, SnippetState>,
    stats: State<'_, StatsState>,
    focus: State<'_, FocusContext>,   // resolves the per-app style + UIPI/elevation check (per-app-context.md)
    events: tauri::ipc::Channel<DictationEvent>,
) -> Result<DictationResult, String>;

/// Abort: discard the in-flight session and any pending STT/injection; HUD → Idle. Idempotent.
#[tauri::command]
fn cancel_dictation(
    app: AppHandle,
    capture: State<'_, CaptureState>,
    focus: State<'_, FocusContext>,   // clears the captured focus target
    events: tauri::ipc::Channel<DictationEvent>,
) -> Result<(), String>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DictationResult {
    injected_chars: usize,
    detected_language: Option<String>,
    total_ms: u64,                          // end-to-end (capture start → injection done)
    stt_ms: u64,                            // STT inference portion
    backend: String,                        // the injection backend actually used ("enigo" | "clipboard"), as returned by inject()
}
```

`Err(String)` paths (each maps to a HUD error state, Section 6/7): `start_dictation` can return
`"no input device available"` (no mic / permission) and `"capture already in progress"` (re-entry,
Rule 12) when it fails to open capture; the tail in `stop_dictation` can return `"model not
downloaded"` (gate, Rule 9), `"stt engine failed: {detail}"`, and `"injection failed: {detail}"`.
Empty/silent speech and user-escape are **not** errors — they resolve to
`DictationResult { injected_chars: 0, .. }` plus a `Cancelled` event (Rules 7-8).

**Pure helpers** (no I/O, `#[cfg(test)]` cargo tests) that live in or are exercised by this module:
the `Phase` state-machine transition function `next_phase(phase, signal) -> Phase`; the
`CancelReason` classifier; the latency-summary builder that assembles `DictationResult` from stage
timestamps; the trigger-mode interpreter that turns a hotkey-down during an active session into
`Continue | Stop | Reject`. Stage internals (VAD constants, whisper arg builder, cleanup rules,
injection backend selection) are tested in their own modules — see the sub-specs.

**Typed UI wrapper**: `app/src/lib/dictation.ts` exposes `startDictation(mode, onEvent)`,
`stopDictation()`, `cancelDictation()` over `invoke(...)` + the `Channel`. The UI holds **no**
dictation logic — it forwards hotkey/tray intent in and renders HUD state out.

---

## 3. Business Rules

1. **Hotkey-down starts a session.** A registered push-to-talk down event ([hotkeys.md](hotkeys.md))
   transitions `Idle → Listening`, shows the HUD, and starts `cpal` capture into the session
   buffer. If a session is already active, see Rule 12.
2. **Push-to-hold endpoints on release.** In `PushToHold`, releasing the hotkey calls
   `stop_dictation`, which stops capture and moves `Listening → Transcribing`. The trailing
   release artifact is trimmed by VAD/cleanup, never injected as text.
3. **Press-to-toggle endpoints on the second press only.** In `PressToToggle`, a second
   hotkey-down stops capture. A session **never** auto-ends on a pause in speech — silence does not
   stop a dictation (the user may pause to think mid-utterance); only an explicit second press ends
   it. (Push-to-hold ends on release, Rule 2.)
4. **VAD gates transcription.** Capture is fed through Silero VAD ([audio-capture.md](audio-capture.md)).
   If no speech segment is detected for the whole session, the session resolves as **empty**
   (Rule 7) — STT is not invoked.
5. **STT uses the warm resident model.** Transcription runs against the already-running, warm
   `whisper-server` sidecar (the cmake-free MVP default; `whisper-rs` in-process is a later
   optimization), with the **fixed** anti-hallucination defaults — Silero VAD + greedy decoding sent
   per `/inference` request with `temperature 0` and `temperature_inc 0` (disables whisper's
   temperature-fallback ladder, the equivalent of `--no-fallback`), and each request is stateless
   with no cross-utterance context conditioning (the equivalent of `--max-context 0`)
   ([ADR-007](architecture.md#adr-007-on-demand-model-download--cpu-bundled--optional-cuda-engine),
   [speech-to-text.md](speech-to-text.md)). `dictation.rs` never cold-spawns a per-utterance CLI
   ([ADR-004](architecture.md#adr-004-warmresident-stt-for-live-dictation)).
6. **Cleanup always runs before injection.** The raw transcript passes through the deterministic
   cleanup module ([text-cleanup.md](text-cleanup.md)) — filler stoplist, spoken-punctuation
   substitution, stutter collapse, whitespace + capitalization — before any text reaches `inject.rs`.
   This is unconditional in Phase 1; LLM Polish ([ai-commands.md](ai-commands.md)) is Phase 2 and
   opt-in.
7. **Empty / silent input injects nothing.** If VAD found no speech, or STT returns empty/whitespace,
   or cleanup reduces the text to empty, MIA injects **no** characters, emits
   `Cancelled { reason: EmptySpeech }`, and returns to `Idle`. It must never emit hallucinated or
   filler-only text.
8. **Escape / abort discards everything.** A `cancel_dictation` (Escape key or HUD/tray abort) at any
   phase stops capture, sets the cancel flag, drops in-flight STT/injection work, injects nothing,
   emits `Cancelled { reason: UserEscape }`, and returns to `Idle`. Idempotent and safe to call when
   already `Idle`.
9. **No model = gated, with a download prompt.** If the active STT model is not present on disk,
   `start_dictation` does not capture; it returns `Err("model not downloaded")`, the HUD shows the
   gate, and the user is routed to the download flow ([speech-to-text.md](speech-to-text.md),
   [onboarding.md](onboarding.md)).
10. **Inject at the focused cursor.** Cleaned text is typed into whatever window has focus at
    injection time via `enigo` SendInput, with the clipboard+Ctrl+V fallback for long text
    (save & restore the user's clipboard) ([text-injection.md](text-injection.md)). Phase moves
    `Transcribing → Inserting` for the duration.
11. **No editable target is tolerated, not blocked.** MIA does not (and on Windows cannot reliably)
    detect whether the focused control accepts text. It injects anyway; if the keystrokes land
    nowhere visible (no caret), the session still completes and the HUD shows a brief
    "inserted (N chars)" — the user sees no text and re-focuses. Elevated/UAC windows: synthetic
    input is dropped by the OS unless MIA is elevated; this is surfaced as a one-time hint
    ([ADR-005](architecture.md#adr-005-system-wide-text-injection-on-windows), Rule in Section 7).
12. **Single active session.** While a session is active, a fresh `start_dictation` invocation
    returns `Err("dictation already active")`. A hotkey-*down* during an active session is routed by
    the trigger-mode interpreter, not as a new session: `PressToToggle` treats it as the stop signal;
    `PushToHold` ignores spurious repeats (auto-repeat key events are debounced).
13. **HUD reflects every phase.** Each phase transition emits `StateChanged`; `Listening` also streams
    `Level` for the live waveform. The HUD is a status mirror only — it issues no dictation commands
    beyond the user's abort button (which calls `cancel_dictation`).
14. **Errors never inject.** Any stage failure (mic, STT, injection) emits `Error { message }`, leaves
    nothing typed, and returns the machine to `Idle` (via the transient `Error` HUD state). No panics
    cross the IPC boundary ([ADR-006](architecture.md#adr-006-resultt-string-error-model-across-the-rust--ui-ipc)).
15. **Audio is discarded after the session.** On any terminal transition (Inserting done, Cancelled,
    Error), the session's PCM buffer is dropped. Nothing is persisted to disk
    ([ADR-001](architecture.md#adr-001-native-on-device-privacy-first)).

---

## 4. Options & Defaults

| Option | Type | Range / values | Default | Effect |
|---|---|---|---|---|
| `trigger_mode` | enum | `pushToHold` \| `pressToToggle` | `pushToHold` | how a session starts/ends — see [hotkeys.md](hotkeys.md) |
| `hotkey` | string | `tauri-plugin-global-shortcut` accelerator | `Ctrl+Space` (locked default — see [hotkeys.md](hotkeys.md)) | the PTT binding |
| `language` | enum | `auto` \| `pt` \| `en` \| … | `auto` | passed to STT — see [speech-to-text.md](speech-to-text.md) |
| `show_hud` | bool | — | `true` | whether the floating mic HUD appears while dictating — see [tray-and-hud.md](tray-and-hud.md) |
| `play_cue` | bool | — | `true` | subtle start/stop audio cue (does not feed the mic buffer) |

STT anti-hallucination defaults (VAD on, greedy, `temperature 0` + `temperature_inc 0` to disable
the fallback ladder, stateless per-request inference so there is no cross-utterance context) are
**fixed and not user-tunable** ([ADR-007](architecture.md#adr-007-on-demand-model-download--cpu-bundled--optional-cuda-engine)).
The UI disables the start action when no model is present (Rule 9); the engine re-checks defensively
and still returns `Err("model not downloaded")`.

---

## 5. Threading / Performance

Live dictation is latency-critical; the design keeps the slow work off the real-time path.

- **Audio thread**: the `cpal` input callback runs on its own real-time thread and only copies
  samples into a lock-free ring buffer. **No** VAD inference, STT, injection, allocation-heavy work,
  or disk logging happens inside the callback ([audio-capture.md](audio-capture.md)).
- **VAD / accumulation thread**: drains the ring buffer, runs Silero VAD, and appends speech frames
  to the session `Vec<f32>`. It emits throttled `Level` events for the HUD waveform (e.g. ~20 Hz),
  not per-frame.
- **STT runs on a blocking task**: transcription against the **warm** `whisper-server` sidecar
  ([ADR-004](architecture.md#adr-004-warmresident-stt-for-live-dictation)) runs in
  `tokio::task::spawn_blocking` (or a dedicated worker) so it never blocks the async runtime or the
  audio thread. The server is started **once** with the model loaded and kept resident — this
  feature does **not** cold-spawn a CLI per utterance.
- **Latency budget** — target **perceived** latency from utterance-end (hotkey release) to first
  injected character: **≈ 1–2 s on the bundled CPU build for a short phrase**, sub-second with the
  optional CUDA engine ([speech-to-text.md](speech-to-text.md)). Where time goes:
  - Capture stop (on release / 2nd toggle press): immediate (off the hot path).
  - **STT inference: the dominant cost** (~80–90% of the wait). Bounded by model size and CPU vs GPU.
  - Cleanup: sub-millisecond (pure string ops, [text-cleanup.md](text-cleanup.md)).
  - Injection: a few ms for SendInput; the clipboard fallback adds a save/paste/restore round-trip
    for long text ([text-injection.md](text-injection.md)).
  Off the hot path: model load (warmed once), audio device open (opened on first session, kept warm
  while the app runs), HUD render (separate webview window). Streaming partials and GPU keep-warm
  sub-second are Phase 5 ([../ROADMAP.md](../ROADMAP.md)).
- **Cancellation**: `cancel_dictation` sets `cancel: AtomicBool`. The flag is checked at every stage
  boundary (after capture, before STT, after STT/before inject) so an Escape promptly stops capture,
  abandons a queued or running transcription where the backend allows, and guarantees no stale text
  is injected (Rules 8, 14). There is **no** session-length cap and **no** silence timeout — a
  session runs until the user ends it (release / 2nd toggle press); the only safety net is the
  push-to-hold missing-release watchdog (`MAX_HOLD_MS`, [hotkeys.md](hotkeys.md)).
- **Resource use**: warm Whisper model RAM per [speech-to-text.md](speech-to-text.md) (CPU build vs
  CUDA). The Phase 2 LLM (llama.cpp, Qwen2.5-3B / Llama-3.2-3B at Q4_K_M ≈ 1.5–2 GB) is **not**
  loaded by this path and only spins up on an AI-command intent match ([ai-commands.md](ai-commands.md)).

---

## 6. UI States

The **floating mic HUD** (dark, translucent, always-on-top) owns the live state; the Settings/Hub
window (light theme) only exposes options/stats. See [tray-and-hud.md](tray-and-hud.md) and
[design-system.md](design-system.md).

```
States: Idle(hidden) → Listening(pulsing waveform) → Transcribing(spinner)
        → Inserting(brief check) → Idle | Error(message) → Idle
Transitions:
  hotkey down            → Listening      (HUD appears; cpal capture starts)
  release | 2nd press    → Transcribing   (capture stops; warm STT runs)
  STT text → cleanup     → Inserting      (SendInject at cursor)
  injection done         → Idle           (HUD fades out; emit Injected)
  escape/abort           → Idle           (Cancelled{UserEscape})
  empty / silent         → Idle           (Cancelled{EmptySpeech}, no inject)
  any stage failure      → Error → Idle   (Error{message})
```

- **HUD per state**: `Listening` shows the live waveform / level meter driven by `Level` events with
  the signature action-blue "listening" pulse; `Transcribing` shows a spinner; `Inserting` a brief
  check; `Error` a short message. Idle = hidden. The single action color and click-through-where-
  possible discipline hold ([design-system.md](design-system.md)).
- **Settings/Hub**: surfaces the options in Section 4 and session stats (words dictated, avg latency)
  per [settings.md](settings.md). No live dictation state lives here.
- Hit targets (the HUD abort control) ≥ 40px; state is conveyed by icon + label, not color alone.

---

## 7. Edge Cases

| Scenario | Expected behavior |
|---|---|
| No microphone / permission denied | `Err("no input device available")`; HUD error; route to [onboarding.md](onboarding.md) |
| Model not yet downloaded | gate the start, `Err("model not downloaded")`, prompt download — [speech-to-text.md](speech-to-text.md), [onboarding.md](onboarding.md) (Rule 9) |
| Silence / VAD finds no speech | no injection, `Cancelled{EmptySpeech}`, back to Idle — no empty/hallucinated text (Rules 4, 7) |
| STT returns whitespace / only fillers after cleanup | treated as empty — inject nothing (Rule 7) |
| Hotkey released mid-transcription | release is the endpoint *cause*; STT is already running on the captured buffer — finish it; never inject stale or partial-of-a-different-utterance text (Rule 2) |
| Escape during Transcribing/Inserting | abandon in-flight work, inject nothing, `Cancelled{UserEscape}` (Rule 8) |
| Focused window is elevated (UAC) | injection silently dropped by OS unless MIA elevated; surface a one-time hint — [text-injection.md](text-injection.md), [ADR-005](architecture.md#adr-005-system-wide-text-injection-on-windows) |
| No editable target (focus on a non-text control / desktop) | inject anyway; session completes; user sees nothing typed and re-focuses (Rule 11) |
| Clipboard fallback used for long text | save the user's prior clipboard, paste, then restore it (Rule 10, [text-injection.md](text-injection.md)) |
| Long pause mid-utterance (toggle) | session keeps recording — silence never auto-ends it; only a 2nd press stops it (Rule 3) |
| Second hotkey-down while active | toggle mode → stop; hold mode → debounced/ignored; new `start_dictation` → `Err("dictation already active")` (Rule 12) |
| Focus changes between capture and injection | text lands in whatever is focused at injection time (by design — live cursor, Rule 10) |
| STT/injection error | `Error{message}`, nothing typed, machine → Idle (Rule 14) |

---

## 8. Testing Checklist

- **Rust** (`cargo test`, pure helpers, no I/O):
  - [x] `next_phase(phase, signal)` covers every transition in Section 6, including illegal
        signals (e.g. release while Idle) resolving to a no-op.
  - [x] trigger-mode interpreter: hotkey-down during an active session → `Stop` (toggle) /
        `Ignore` (hold), `Start` when idle (Rule 12; the already-active `Reject` is command-level).
  - [x] `CancelReason` classifier (UserEscape vs EmptySpeech vs Timeout).
  - [x] empty-result handling: the `Empty`/`TranscribedEmpty` signals return to Idle and map to
        `EmptySpeech` (Rule 7). (The per-stage emptiness checks live in cleanup/stt.)
  - [x] `DictationResult` latency-summary builder from stage timestamps.
  - [x] each `Err(String)` the commands return is wired (model-not-downloaded via `warm_model`, STT
        failure, injection failure; "capture already in progress" guards re-entry) — build-verified.
- **Manual / runtime** (needs mic, model, real focused app):
  - [ ] happy path push-to-hold: hold → speak → release → text at cursor (pt-BR and English).
  - [ ] happy path press-to-toggle: press → speak (pausing mid-sentence does not stop it) → press again → text at cursor.
  - [ ] HUD reflects every state (listening waveform / transcribing / inserting / error).
  - [ ] Escape during Listening and during Transcribing both inject nothing and return to Idle.
  - [ ] silent session injects nothing (no hallucination).
  - [ ] model-not-downloaded gate routes to download (Rule 9).
  - [ ] injection into multiple targets (browser, code editor, chat box); clipboard restored after
        the long-text fallback.
  - [ ] elevated/UAC window: hint surfaced, no crash.
  - [ ] end-to-end perceived latency within budget on the CPU build (Section 5).

---

## 9. Out of Scope (this version)

- **Streaming live partials** — Phase 1 transcribes the full utterance after endpoint; incremental
  on-screen partials (and the `Partial` event that would carry them) are Phase 5
  ([../ROADMAP.md](../ROADMAP.md)).
- **Wake word ("Hey MIA") / always-on listening** — only explicit hotkey/tray triggers in v1; Phase 5.
- **AI Command Mode & Polish** — voice editing and LLM rewriting are Phase 2 and opt-in, off the
  hot path ([ai-commands.md](ai-commands.md), [ADR-008](architecture.md#adr-008-hybrid-text-intelligence)).
- **GPU keep-warm sub-second** target — Phase 5; CUDA engine itself ships in Phase 4
  ([speech-to-text.md](speech-to-text.md)).
- **Target-window selection / smart per-app behavior** — injects at the live cursor only; per-app
  writing styles are Phase 3 ([custom-dictionary.md](custom-dictionary.md), [snippets.md](snippets.md)).
- **File-transcription mode** — MIA is live-only; the Toolzy ffmpeg+file path is backlog
  ([REUSE-FROM-TOOLZY.md](../REUSE-FROM-TOOLZY.md), [../ROADMAP.md](../ROADMAP.md)).
- **macOS / Linux** — Windows-only v1 ([ADR-011](architecture.md#adr-011-windows-only-v1)).
