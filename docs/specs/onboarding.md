# Onboarding Feature Spec

> **Status**: Draft / Planned (Phase 0 — docs being written; no code exists yet)
> **Last updated**: 2026-05-28
> **Coverage**: Sections 1-9 drafted.
> **Environment**: desktop (Windows, native)

First-run onboarding is the guided wizard MIA shows the very first time it launches (and on demand
later from [settings.md](settings.md)). It walks the user from a cold install to a working dictation
loop in five steps: welcome + privacy promise → microphone permission → pick & download the first
Whisper model → set the push-to-talk hotkey → a guided first dictation. When the wizard finishes, the
window closes and the app lives in the system tray, ready for global push-to-talk. Onboarding is the
**front door** to the dictation pipeline (hotkey → capture → VAD → STT → cleanup → inject) — it does
not run dictation itself, but it provisions and validates every prerequisite that pipeline needs. It
lands in **Phase 4 — Polish & Distribution** (see [../ROADMAP.md](../ROADMAP.md)), though the model
download gate it depends on ships in Phase 1. It implements **ADR-001** (privacy-first, everything
local), **ADR-007** (on-demand model download + optional CUDA engine), and reuses Toolzy's download
gate UX (see [../REUSE-FROM-TOOLZY.md](../REUSE-FROM-TOOLZY.md)).

**Scope decisions** (locked at design time):

- **Linear wizard with five named steps** (Welcome → Mic → Model → Hotkey → TryIt → Done), back/next
  navigation allowed, but the **Model** step cannot be skipped — dictation is impossible without at
  least one model on disk (ADR-007 / Phase 4).
- **Privacy is the first thing the user reads.** The Welcome step states plainly that voice never
  leaves the machine: no cloud, no account, no server (ADR-001). This is the product's whole pitch
  versus [Wispr Flow](https://wisprflow.ai); we lead with it.
- **MIA cannot grant the OS microphone permission for the user** — Windows owns that toggle. Onboarding
  *detects* the permission state and *guides* the user to the Windows Settings page; it never claims to
  flip it silently (ADR-001 — no hidden privileged behavior).
- **Latency-friendly default model.** We pre-select **`small`** for the first download — the best
  accuracy/latency trade for live dictation on a CPU build — and clearly label sizes/RAM so the user
  can choose `medium` (slower, more accurate) or `base` (faster, lighter). Defaults and rationale live
  in [speech-to-text.md](speech-to-text.md) (ADR-007).
- **GPU is an optional offer, never a blocker.** The NVIDIA CUDA engine step only appears if a GPU is
  detected (`nvcuda.dll` present); on no-GPU machines the step is skipped silently and MIA runs the
  bundled CPU build (ADR-007).
- **No account, no telemetry, no network beyond model/engine downloads.** The only outbound traffic
  during onboarding is the Hugging Face model fetch and the optional CUDA engine fetch (ADR-001).
- **Re-runnable.** Onboarding is not a one-shot — it can be re-launched from
  [settings.md](settings.md) ("Re-run setup") and each step independently re-checks live state, so it
  doubles as a diagnostics/repair flow.

---

## 1. Inputs / Outputs

Onboarding is a provisioning wizard, not a dictation feature — it consumes UI events and OS state and
produces persisted configuration plus on-disk assets. The single exception is the **TryIt** step,
which runs one real dictation pass through the live pipeline.

| Aspect | This feature |
|---|---|
| **Trigger** | First app launch (no completed-onboarding flag in config); or manual "Re-run setup" from the Hub |
| **Audio in** | None for steps 1-4. **TryIt** step: cpal 16 kHz mono PCM f32 stream (the real capture path) |
| **Text in** | None (no transcript consumed); TryIt's transcript is shown for confirmation only, not injected by default |
| **Text out** | Persisted config (chosen model, hotkey binding, engine = CPU/CUDA, `onboardingCompleted=true`); a downloaded model file (and optional CUDA engine) in app-data |
| **Target** | The onboarding window (Settings/Hub light theme); the system tray (post-finish residence); Windows Settings deep-link (mic privacy page) |
| **Language** | UI: pt-BR + English (first-class, follows app locale). TryIt transcription: the user's selected dictation language (auto-detect default) |

Backing crates/engines: model + CUDA download reuse Toolzy's `transcription.rs` (`ureq` HTTP, `.part`
rename-on-complete, progress `Channel`); mic enumeration/capture via **cpal**; hotkey recording via
**global-hotkey**; tray residence via **tray-icon**; GPU detection via probing for `nvcuda.dll`. The
audio buffer in the TryIt step lives **in memory only** and is discarded after the step — it never
touches disk (ADR-001).

---

## 2. Engine Contract (Rust)

**Module**: `app/src-tauri/src/onboarding.rs` (orchestration + config flag), delegating to
`audio.rs` (mic enumeration/permission probe), `transcription.rs` / `speech_to_text.rs` (model registry
+ download + GPU detect — see [speech-to-text.md](speech-to-text.md)), and `hotkeys.rs` (binding capture
+ persistence — see [hotkeys.md](hotkeys.md)). The Svelte wizard in `app/src/lib/onboarding.ts` holds
only step-navigation state; all checks/downloads/persistence happen in Rust. All commands return
`Result<T, String>` (ADR-006).

```rust
// Wizard lifecycle ----------------------------------------------------------
#[tauri::command]
async fn onboarding_status() -> Result<OnboardingStatus, String>;
// { completed: bool, hasModel: bool, micState: MicState, gpuAvailable: bool }

#[tauri::command]
async fn complete_onboarding(state: State<'_, AppState>) -> Result<(), String>;
// Persists onboardingCompleted=true; idempotent.

// Microphone step -----------------------------------------------------------
#[tauri::command]
async fn list_input_devices() -> Result<Vec<InputDevice>, String>; // cpal enumeration
#[tauri::command]
async fn check_mic_permission() -> Result<MicState, String>;
// MicState = Granted | Denied | Unknown | NoDevice (best-effort probe; Windows owns the truth)
#[tauri::command]
async fn open_windows_mic_settings() -> Result<(), String>;
// Opens ms-settings:privacy-microphone via the OS shell. Does NOT change the setting.

// Model step (reuses Toolzy transcription.rs) -------------------------------
#[tauri::command]
fn list_models() -> Result<Vec<ModelInfo>, String>;
// { id, sizeBytes, ramEstimateMb, recommended, downloaded } from the MODELS registry.
#[tauri::command]
async fn download_model(
    state: State<'_, AppState>,
    model_id: String,
    on_progress: tauri::ipc::Channel<DownloadProgress>,
) -> Result<(), String>; // .part → rename on complete; resumable; cancelable via managed State
#[tauri::command]
async fn cancel_download(state: State<'_, AppState>) -> Result<(), String>;

// Hotkey step (see hotkeys.md) ----------------------------------------------
#[tauri::command]
async fn record_hotkey(on_capture: tauri::ipc::Channel<HotkeyChord>) -> Result<(), String>;
#[tauri::command]
async fn set_dictation_hotkey(state: State<'_, AppState>, chord: HotkeyChord) -> Result<(), String>;
// Err if the chord is already claimed by another binding or fails to register globally.

// GPU step (optional; reuses Toolzy CUDA detect/download) --------------------
#[tauri::command]
async fn detect_gpu() -> Result<GpuInfo, String>; // { available: bool, name: Option<String> }
#[tauri::command]
async fn download_cuda_engine(
    state: State<'_, AppState>,
    on_progress: tauri::ipc::Channel<DownloadProgress>,
) -> Result<(), String>;

// TryIt step — one real dictation pass, transcript returned, NOT injected ----
#[tauri::command]
async fn onboarding_try_dictation(
    state: State<'_, AppState>,
    on_event: tauri::ipc::Channel<TryItEvent>, // level meter → transcribing → result
) -> Result<String, String>; // returns the cleaned transcript for confirmation only
```

- All `*Opts` / `*Result` structs use serde `rename_all = "camelCase"`.
- `Err(String)` messages map 1:1 to UI error states — e.g. `"no input device"`, `"mic permission denied"`,
  `"download interrupted"`, `"hotkey already in use: <chord>"`, `"model not downloaded"`.
- The TryIt pass uses the **warm** model (ADR-004), not a cold `whisper-cli` spawn — onboarding loads the
  just-downloaded model into the resident whisper-rs instance and reuses it (this is also a real warm-up
  so the first post-onboarding dictation is fast).
- **Pure helpers** (behind `#[cfg(test)]`, no I/O): the `MODELS` registry lookup + `recommended` flag,
  `model_url`/`model_filename` builders, the size/RAM formatter, the `HotkeyChord` parse/serialize round-trip,
  and the `MicState`/`GpuInfo` mapping from probe results to UI states.
- Typed UI wrapper: `app/src/lib/onboarding.ts` (`invoke<OnboardingStatus>("onboarding_status", …)`, etc.).

---

## 3. Business Rules

1. **First-launch detection** — On startup, if config has no `onboardingCompleted=true` flag, MIA opens
   the onboarding window instead of going straight to the tray. With the flag set, MIA boots directly to
   the tray.
2. **Privacy promise on Welcome** — The Welcome step must visibly state that everything runs locally:
   no cloud, no account, no server, voice never leaves the device (ADR-001). Next is the only action.
3. **Mic state is detected, never asserted** — `check_mic_permission` returns a best-effort `MicState`.
   The step shows: Granted → green check, allow Next; Denied/Unknown → a guide with an "Open Windows
   microphone settings" button (`open_windows_mic_settings`) and a "Re-check" button; NoDevice → "No
   microphone found" with a Re-check.
4. **MIA never silently changes a Windows privacy setting** — `open_windows_mic_settings` only deep-links
   to `ms-settings:privacy-microphone`; the user flips the toggle themselves (ADR-001).
5. **Mic step can proceed on Unknown** — Because Windows may report `Unknown` even when capture works,
   the user may proceed past a non-Granted state with an explicit "Continue anyway" affordance; the TryIt
   step is the real test. Only `NoDevice` hard-blocks Next.
6. **The Model step is mandatory** — Next is disabled until at least one model is `downloaded`. There is
   no skip. (Dictation is impossible without a model; ADR-007.)
7. **`small` is pre-selected and labeled "Recommended"** — Each option shows download size and estimated
   RAM. The user may switch to `base` (faster/lighter), `medium`, or larger (slower/more accurate) before
   downloading. Definitions live in [speech-to-text.md](speech-to-text.md).
8. **Download is gated, streamed, resumable, and cancelable** — `download_model` streams
   `DownloadProgress` over a `Channel`, writes to a `.part` file, renames on success (Toolzy pattern),
   and is cancelable via `cancel_download`. A partial/interrupted download leaves no half-file registered
   as complete (the `.part` is never renamed) and can be retried/resumed.
9. **Anti-hallucination STT defaults are fixed** — Onboarding never exposes decoding knobs (VAD, greedy,
   temperature 0, `--max-context 0` are always on; ADR-007). The model size is the only STT choice here.
10. **Hotkey must register globally before Next** — `set_dictation_hotkey` must succeed (chord parses and
    `global-hotkey` registers it system-wide). If the chord is already claimed (by MIA or another app),
    return `Err("hotkey already in use: <chord>")` and keep the recorder open. A sensible default is
    pre-filled (see [hotkeys.md](hotkeys.md)) so the user can simply accept it.
11. **TryIt is guided and forgiving** — The step instructs "Hold your hotkey and say *hello*", runs one
    real capture→VAD→STT→cleanup pass via `onboarding_try_dictation`, and shows the resulting transcript
    for confirmation. By default it does **not** inject (the focus is the wizard, not another app). The
    user may "Try again" any number of times or "Skip" to finish.
12. **Silence in TryIt is not a failure** — If VAD detects no speech, show "We didn't hear anything —
    hold the key and speak" and return to ready (no empty/hallucinated transcript; mirrors the pipeline's
    silence rule).
13. **GPU step is conditional** — It appears only if `detect_gpu().available == true`. It offers the
    optional CUDA engine (~7-10x faster) with its download size; declining keeps the bundled CPU build.
    On no-GPU machines the step is skipped entirely and is invisible.
14. **Finish persists everything and goes to tray** — On Done, `complete_onboarding` sets
    `onboardingCompleted=true`; the window closes; the tray icon is the app's resident presence
    (see [tray-and-hud.md](tray-and-hud.md)). The chosen model is left warm for the first real dictation.
15. **Re-runnable & idempotent** — Re-running onboarding from the Hub re-probes live state at each step;
    already-downloaded models show as installed (no re-download), and `complete_onboarding` is idempotent.

---

## 4. Options & Defaults

| Option | Type | Range / values | Default | Effect |
|---|---|---|---|---|
| `selectedModel` | enum | `base` / `small` / `medium` / `large-v3` (per registry) | `small` | Which Whisper model to download as the first model; accuracy↔latency↔RAM trade |
| `inputDevice` | enum | enumerated cpal input devices | system default | Mic used for the TryIt step and later dictation |
| `dictationHotkey` | chord | any valid `global-hotkey` chord | platform default (see [hotkeys.md](hotkeys.md)) | The global push-to-talk binding |
| `useCudaEngine` | bool | true / false | `false` (offered only if GPU detected) | Use the downloaded NVIDIA CUDA engine vs. bundled CPU build |
| `tryItInjects` | bool | true / false | `false` | Whether the TryIt transcript is typed into the focused app (off — keeps focus on the wizard) |
| `onboardingCompleted` | bool | true / false | `false` | Gate that decides whether the wizard runs at next launch |

Validation split: the **UI** disables Next until each step's precondition holds (model downloaded,
hotkey registered, mic not `NoDevice`); the **engine** re-checks defensively on each command
(`download_model` refuses an unknown id, `set_dictation_hotkey` re-verifies registration,
`complete_onboarding` no-ops if already complete). STT decoding defaults are **fixed**, not surfaced
here (ADR-007).

---

## 5. Threading / Performance

- **Downloads run off the UI**: `download_model` and `download_cuda_engine` run as async Tauri commands
  on the runtime's worker pool, streaming `DownloadProgress` over a `Channel` so the wizard shows a live
  percentage/byte counter without blocking. Cancellation flips a flag in managed `State`; the writer
  stops and the `.part` file is left for resume/retry (Toolzy pattern).
- **Mic / GPU probes are cheap and synchronous-ish**: device enumeration and `nvcuda.dll` detection are
  fast best-effort checks; run them async to avoid any UI stall and cache the result per step entry.
- **TryIt uses the real audio + warm-model path** (ADR-004): the cpal callback runs on its own real-time
  thread and hands samples to the engine via a channel — no STT/injection/disk-logging in the callback.
  The model is loaded **once** (the download we just completed) into the resident whisper-rs instance and
  kept warm; the TryIt pass is also the warm-up for the first real dictation. Onboarding never cold-spawns
  `whisper-cli`.
- **Latency budget**: onboarding itself is not latency-critical (it's a wizard), but the TryIt step should
  feel like real dictation — utterance-end → transcript shown within the model's normal inference window;
  the dominant cost is STT inference, which is why we warm the model rather than spawn per pass.
- **Cancellation**: leaving the Model step mid-download cancels and retains the `.part`; leaving the TryIt
  step mid-capture stops the cpal stream and discards the in-memory buffer (nothing persisted).
- **Resource use**: model RAM follows the chosen size (CPU build vs. CUDA); the LLM is **not** involved in
  onboarding. Only the selected model is loaded; the CUDA engine is downloaded only on explicit opt-in.

---

## 6. UI States

Onboarding lives entirely in the **Settings/Hub window (light "Calm Focus" theme)** — it is a webview
wizard, not a HUD overlay. The TryIt step is the only place the dark **floating mic HUD** may briefly
appear (it is the real capture path). See [tray-and-hud.md](tray-and-hud.md) and
[design-system.md](design-system.md).

```
Wizard:  Welcome ──▶ Mic ──▶ Model(download) ──▶ Hotkey ──▶ TryIt ──▶ Done ──▶ (window closes → tray)
                      ▲        │ (mandatory)        ▲          │
                      └────────┘ Back allowed       └──────────┘   GPU step inserted before Done iff gpuAvailable

Mic step:    Checking → Granted | Denied/Unknown(guide + open-settings + recheck) | NoDevice(block)
Model step:  Choose → Downloading(% bar, cancelable) → Installed(check) | DownloadError(retry/resume)
Hotkey step: Idle(default prefilled) → Recording(capturing chord) → Bound(check) | Conflict(error, re-record)
TryIt step:  Ready("hold the key & say hello") → Listening(waveform) → Transcribing(spinner)
             → Result(transcript shown) | NoSpeech("didn't hear anything") → Ready
```

- **Settings/Hub (light)**: a left rail or stepper showing the five (or six) steps with check marks for
  completed ones; a single primary **action-blue** button (Next / Download / Finish) per step; Back as a
  secondary control. Empty/loading/error states per step as above (spinner while checking mic/GPU,
  progress bar while downloading, inline error cards with a retry affordance).
- **HUD (TryIt only)**: the dark translucent pill with the action-blue pulsing waveform during Listening
  and a spinner during Transcribing — the same component used in live dictation, so the user sees exactly
  what they'll see later.
- Maintain the **one-action-color** discipline (action-blue #006BFF is the only action color), ≥40px hit
  targets, visible focus rings, and never rely on color alone (mic-granted shows a check icon + label, not
  just green). Wizard is keyboard-navigable: Enter = Next, Esc = Back where non-destructive.

---

## 7. Edge Cases

| Scenario | Expected behavior |
|---|---|
| Microphone permission denied | Mic step shows guide + "Open Windows microphone settings" (`ms-settings:privacy-microphone`) + "Re-check"; never claims to flip it; TryIt is the real test |
| No microphone device at all | `MicState::NoDevice` → "No microphone found", Next blocked until a device appears and Re-check passes |
| Windows reports `Unknown` permission | Allow "Continue anyway"; the TryIt capture is the ground truth |
| Model download interrupted (network drop / quit) | `.part` file kept (never renamed); show "Download interrupted — Retry"; resume/retry from where possible (Toolzy pattern); no corrupt model registered |
| User tries to skip the Model step | Not allowed — Next stays disabled until ≥1 model is downloaded (ADR-007) |
| Hotkey chord already in use | `Err("hotkey already in use: <chord>")`; recorder stays open; suggest the default or another chord |
| No NVIDIA GPU | GPU step is skipped silently and never shown; MIA uses the bundled CPU build |
| CUDA engine download fails | Keep CPU build; show a non-blocking "couldn't install GPU engine — using CPU" note; user can retry later from the Hub |
| Silence during TryIt (VAD: no speech) | "We didn't hear anything — hold the key and speak"; return to Ready; no empty/hallucinated transcript |
| Focused window during TryIt is elevated (UAC) | Irrelevant by default (TryIt doesn't inject); if `tryItInjects` is on, injection may fail silently unless MIA is elevated (ADR-005) — surface it |
| Disk full / app-data not writable during download | `Err("download interrupted")` with the cause; prompt to free space and retry; no partial file left registered |
| User closes the window mid-wizard | `onboardingCompleted` stays false → wizard re-opens next launch from where it makes sense (re-probes live state) |
| Re-run from the Hub with model already present | Model shows "Installed" (no re-download); steps re-validate live state; Finish is idempotent |

---

## 8. Testing Checklist

- **Rust** (`cargo test`, no I/O — pure helpers only):
  - [ ] `MODELS` registry lookup + `recommended` flag (= `small`), `model_url`/`model_filename` builders
  - [ ] size/RAM label formatter for each model size
  - [ ] `HotkeyChord` parse ↔ serialize round-trip; conflict-detection logic
  - [ ] `MicState` / `GpuInfo` mapping from probe inputs to UI states (Granted/Denied/Unknown/NoDevice)
  - [ ] each `Err(String)` path: unknown model id, hotkey-in-use, model-not-downloaded, download-interrupted
  - [ ] `complete_onboarding` idempotency (no-op when already complete)
- **Manual / runtime** (needs mic, network, and ideally a GPU machine + a non-GPU machine):
  - [ ] cold first launch opens the wizard; second launch boots straight to tray
  - [ ] mic denied → open-settings deep-link works → re-check flips to Granted
  - [ ] model download shows live progress, cancel leaves a `.part` and no registered model, retry/resume works
  - [ ] hotkey recorder captures a chord, rejects a conflicting one, registers globally
  - [ ] TryIt: hold key, say "hello" → transcript shown (pt-BR and English); silence shows the no-speech message
  - [ ] GPU machine: CUDA offer appears and installs; non-GPU machine: step is absent, CPU build used
  - [ ] Finish closes window, tray icon present, first real dictation is fast (model stayed warm)
  - [ ] "Re-run setup" from the Hub re-probes state and shows installed model as installed

---

## 9. Out of Scope (this version)

- **Account / sign-in / cloud sync of settings** — MIA has no account by design (ADR-001); never added.
- **Multi-language UI picker beyond pt-BR/English** — the wizard follows the two first-class UI locales;
  broader UI localization is backlog. (Dictation itself covers ~99 Whisper languages — see
  [speech-to-text.md](speech-to-text.md).)
- **Per-app writing styles / custom dictionary / snippets setup** — personalization is Phase 3
  ([custom-dictionary.md](custom-dictionary.md), [snippets.md](snippets.md)); onboarding gets the user
  dictating first, not configuring vocabulary.
- **LLM "Polish" / Command Mode setup** — Phase 2 ([ai-commands.md](ai-commands.md)); the optional local
  LLM is downloaded/enabled later from the Hub, not during first-run, to keep onboarding fast and small.
- **macOS / Linux permission flows** (Accessibility/TCC, Wayland) — Windows-only v1 (ADR-011); deferred
  to Phase 5 / Backlog (see [../ROADMAP.md](../ROADMAP.md)).
- **Importing/migrating settings from Toolzy or other tools** — backlog.
