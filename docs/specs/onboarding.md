# Onboarding Feature Spec

> **Status**: Phase 4 — first-run wizard implemented (`Onboarding.svelte`, build-verified): welcome → hotkey (shows the chord via `get_hotkey`) → mic test (`test_microphone`) → model download. The Model step lists **all four registry models with sizes** (`small` flagged "Recomendado"), mirroring the Hub; it is **mandatory** — there is no skip and "Concluir" stays disabled until a model is on disk (Rule 6/7). `App.svelte` shows the wizard only when onboarding hasn't been completed **and** no model is installed; "Concluir" persists `settings.general.onboarding_completed=true` (Rule 1/14) so MIA then boots straight to the Hub. The **permission-denied deep-link is now wired**: a denied mic capture is tagged by `classify_mic_error` (sentinel `mic-permission-denied:`) and the mic-test step surfaces an "Abrir configurações" button that launches `ms-settings:privacy-microphone` (the `open_mic_privacy` command). Live mic level meter during the test.
> **Last updated**: 2026-05-30
> **Coverage**: Sections 1-9 drafted.
> **Environment**: desktop (Windows, native)

First-run onboarding is the guided wizard MIA shows the very first time it launches (and on demand
later from [settings.md](settings.md)). It walks the user from a cold install to a working dictation
loop in four steps: welcome + privacy promise → set the push-to-talk hotkey → microphone test → pick &
download the first Whisper model. When the wizard finishes, the
window closes and the app lives in the system tray, ready for global push-to-talk. Onboarding is the
**front door** to the dictation pipeline (hotkey → capture → VAD → STT → cleanup → inject) — it does
not run dictation itself, but it provisions and validates every prerequisite that pipeline needs. It
lands in **Phase 4 — Polish & Distribution** (see [../ROADMAP.md](../ROADMAP.md)), though the model
download gate it depends on ships in Phase 1. It implements **ADR-001** (privacy-first, everything
local), **ADR-007** (on-demand model download + optional CUDA engine), and reuses Toolzy's download
gate UX (see [../REUSE-FROM-TOOLZY.md](../REUSE-FROM-TOOLZY.md)).

**Scope decisions** (locked at design time):

- **Linear wizard with four named steps** (Welcome → Hotkey → Mic → Model → Done), back/next
  navigation allowed, but the **Model** step cannot be skipped — dictation is impossible without at
  least one model on disk (ADR-007 / Phase 4). _(A guided in-wizard first-dictation "TryIt" step is
  backlog — see §9; the real first dictation happens via the live PTT path after the wizard closes.)_
- **Privacy is the first thing the user reads.** The Welcome step states plainly that voice never
  leaves the machine: no cloud, no account, no server (ADR-001). This is the product's whole pitch
  versus [Wispr Flow](https://wisprflow.ai); we lead with it.
- **MIA cannot grant the OS microphone permission for the user** — Windows owns that toggle. Onboarding
  *detects* the permission state and *guides* the user to the Windows Settings page; it never claims to
  flip it silently (ADR-001 — no hidden privileged behavior).
- **Latency-friendly default model.** We pre-select **`small`** for the first download — the best
  accuracy/latency trade for live dictation on a CPU build — and clearly label sizes/RAM so the user
  can choose `medium` (slower, more accurate) or a larger model. The registry is
  `small` / `medium` / `large-v3-turbo` / `large-v3` (there is no `base`); defaults and rationale live
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

Backing crates/engines: model + CUDA download reuse the engine's **`stt.rs`** (adapted from Toolzy's
`transcription.rs` — `ureq` HTTP, `.part` rename-on-complete, progress `Channel`); mic
enumeration/capture via **cpal** (`audio.rs`); the global PTT hotkey via the
**`tauri-plugin-global-shortcut`** plugin (`hotkey.rs`); tray residence via Tauri's **built-in
tray-icon feature** (`tray.rs`); GPU detection via probing for `nvcuda.dll`. Any audio captured during a
guided first-dictation pass lives **in memory only** and is discarded — it never touches disk
(ADR-001). _(The TryIt-in-wizard pass itself is Phase-pending — see §2.)_

---

## 2. Engine Contract (Rust)

**There is no dedicated `onboarding.rs` module and no `onboarding.ts` wrapper, and onboarding adds
no new `#[tauri::command]`s.** The wizard that shipped (Phase 4) is a **Svelte-only component** —
`app/src/lib/components/Onboarding.svelte`, mounted by `App.svelte` — that **composes existing,
already-registered commands** through their existing typed wrappers. The orchestration that earlier
drafts of this spec attributed to a Rust onboarding engine (a wizard `onboarding_status` /
`complete_onboarding` lifecycle, a dedicated mic-permission probe, GPU detect, a `onboarding_try_dictation`
pass, etc.) was **never implemented** and is **out of scope / Phase-pending**, not the live contract.

**What the shipped wizard actually calls** (all pre-existing commands, all returning `Result<T, String>`,
ADR-006):

| Wizard step | Wrapper used | Underlying command | Module |
|---|---|---|---|
| Hotkey | `getHotkey()` (`app/src/lib/hotkey.ts`) | `get_hotkey` | `hotkey.rs` |
| Microphone test | `testMicrophone(ms)` (`app/src/lib/audio.ts`) | `test_microphone` | `audio.rs` |
| Model download | `listWhisperModels()` / `downloadWhisperModel(id, channel)` (`app/src/lib/stt.ts`) | `list_whisper_models` / `download_whisper_model` | `stt.rs` |

```ts
// app/src/lib/components/Onboarding.svelte (presentation only — no new commands)
import { testMicrophone } from "../audio";
import { getHotkey } from "../hotkey";
import { downloadWhisperModel, listWhisperModels } from "../stt";
// step 1 Welcome  · step 2 Atalho (shows getHotkey().accelerator)
// step 3 Microfone (testMicrophone) · step 4 Modelo (download with a progress Channel)
```

- The four shipped steps are **Welcome → Hotkey → Mic test → Model download** (labels in the component:
  "Bem-vindo", "Atalho", "Microfone", "Modelo"). `App.svelte` shows the wizard when **no Whisper model is
  installed yet** (gated on model presence, not a persisted flag). The Model step is **mandatory — there is
  no skip** (Rule 6): "Concluir" is disabled until at least one model is on disk; only "Voltar" navigates away.
- The hotkey step is **read-only**: it *displays* the current PTT chord via `get_hotkey` (the locked
  `Ctrl+Space` default — `DEFAULT_ACCEL` in `hotkey.rs`); the wizard does not yet record/rebind a chord.
  Rebinding lives in [settings.md](settings.md) / [hotkeys.md](hotkeys.md).
- The mic step calls `test_microphone(ms)` (a short capture probe in `audio.rs`); it does **not** call a
  dedicated permission-probe command — there is none.
- The model step reuses the existing **`stt.rs`** download surface (`list_whisper_models` /
  `download_whisper_model`) with its `.part`-rename + progress-`Channel` UX from
  [speech-to-text.md](speech-to-text.md) — no onboarding-specific download command. It now renders **all four
  registry models with sizes** and per-row download (the same shape as the Hub), `small` flagged "Recomendado"
  (Rule 7); each row shows "Baixar" → "baixando… N%" → "✓ instalado".
- **No TryIt-through-the-engine command exists.** The deeper onboarding-engine commands sketched above
  (lifecycle flag, `check_mic_permission`, `open_windows_mic_settings`, `detect_gpu` for an onboarding
  GPU step, `onboarding_try_dictation`) are **Phase-pending / out of scope** for the shipped wizard.
  The closest live equivalent of a "first dictation" is simply the real PTT path after the wizard closes
  ([dictation.md](dictation.md)); a guided in-wizard TryIt pass and a persisted `onboardingCompleted`
  flag are still pending (see the Status block and §9).

> **Why no engine module.** Onboarding is pure provisioning UX that only needs to *display* the hotkey,
> *probe* the mic, and *trigger* the model download — every one of those is already a registered command
> for the Hub. Per ADR-002, a thin Svelte component composing existing wrappers is the correct shape; a
> parallel `onboarding.rs` would duplicate `hotkey.rs` / `audio.rs` / `stt.rs`.

---

## 3. Business Rules

> **Implemented vs designed.** The shipped Phase-4 wizard implements rules **2** (privacy promise), the
> mic-test spirit of **3/5**, **6/7** (model is required, `small` recommended), and **8** (gated, streamed,
> cancelable download) — all via existing commands (§2). First-launch detection (**1**, currently gated on
> *model presence*, not a persisted flag), the GPU step (**13**), the in-wizard TryIt pass (**11/12**),
> hotkey *rebinding* (**10**), and the `onboardingCompleted` persistence + idempotent `complete_onboarding`
> (**14/15**) are the **designed flow** and remain **Phase-pending** (no engine module backs them yet).

1. **First-launch detection** — On startup, if config has no `onboardingCompleted=true` flag, MIA opens
   the onboarding window instead of going straight to the tray. With the flag set, MIA boots directly to
   the tray. _(Shipped behavior gates on whether a Whisper model is installed; the persisted flag is
   Phase-pending.)_
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
   RAM. The user may switch to `medium`, `large-v3-turbo`, or `large-v3` (slower/more accurate) before
   downloading (there is no `base` in the registry). Definitions live in [speech-to-text.md](speech-to-text.md).
8. **Download is gated, streamed, resumable, and cancelable** — `download_model` streams
   `DownloadProgress` over a `Channel`, writes to a `.part` file, renames on success (Toolzy pattern),
   and is cancelable via `cancel_download`. A partial/interrupted download leaves no half-file registered
   as complete (the `.part` is never renamed) and can be retried/resumed.
9. **Anti-hallucination STT defaults are fixed** — Onboarding never exposes decoding knobs (Silero VAD,
   greedy/temperature-0 decoding with no temperature fallback, and stateless per-utterance recognition
   are always on; ADR-007 — see [speech-to-text.md](speech-to-text.md)). The model size is the only STT
   choice here.
10. **Hotkey must register globally before Next** — _(Phase-pending — the shipped wizard only displays the
    locked `Ctrl+Space` chord; rebinding lives in [settings.md](settings.md) / [hotkeys.md](hotkeys.md).)_
    In the designed flow a recorded chord must parse and register system-wide via
    `tauri-plugin-global-shortcut`. If the chord is already claimed (by MIA or another app),
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
| `selectedModel` | enum | `small` / `medium` / `large-v3-turbo` / `large-v3` (per the `stt.rs` registry — there is no `base`) | `small` | Which Whisper model to download as the first model; accuracy↔latency↔RAM trade |
| `inputDevice` | enum | enumerated cpal input devices | system default | Mic used for the TryIt step and later dictation |
| `dictationHotkey` | chord | any valid `tauri-plugin-global-shortcut` chord | `Ctrl+Space` (locked `DEFAULT_ACCEL`; see [hotkeys.md](hotkeys.md)) | The global push-to-talk binding |
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
- **TryIt would use the real audio + warm-model path** (ADR-004) — _Phase-pending; the shipped wizard
  stops after the model download and lets the user dictate via the live PTT path._ In the designed flow the
  cpal callback runs on its own real-time thread and hands samples to the engine via a channel — no
  STT/injection/disk-logging in the callback. The model is loaded **once** (the download we just completed)
  into the warm **whisper-server** sidecar (MVP default — `whisper-rs` in-process is the later
  optimization) and kept warm; the TryIt pass is also the warm-up for the first real dictation. Onboarding
  never cold-spawns `whisper-cli`.
- **Latency budget**: onboarding itself is not latency-critical (it's a wizard), but the TryIt step should
  feel like real dictation — utterance-end → transcript shown within the model's normal inference window;
  the dominant cost is STT inference, which is why we warm the model rather than spawn per pass.
- **Cancellation**: leaving the Model step mid-download cancels and retains the `.part`; leaving the TryIt
  step mid-capture stops the cpal stream and discards the in-memory buffer (nothing persisted).
- **Resource use**: model RAM follows the chosen size (CPU build vs. CUDA); the LLM is **not** involved in
  onboarding. Only the selected model is loaded; the CUDA engine is downloaded only on explicit opt-in.

---

## 6. UI States

Onboarding lives entirely in the **Settings/Hub window (Blush Playground)** — it is a webview
wizard, not a HUD overlay. The shipped wizard has no in-wizard capture step; the **floating mic HUD**
(a white Blush pill — white, 2px charcoal outline) only appears later during the real PTT path. See
[tray-and-hud.md](tray-and-hud.md) and [design-system.md](design-system.md).

```
Wizard:  Welcome ──▶ Hotkey ──▶ Mic ──▶ Model(download) ──▶ Done ──▶ (window closes → tray)
                                         │ (mandatory)
                                Back allowed   GPU step inserted before Done iff gpuAvailable

Mic step:    Checking → Granted | Denied/Unknown(guide + open-settings + recheck) | NoDevice(block)
Model step:  Choose → Downloading(% bar, cancelable) → Installed(check) | DownloadError(retry/resume)
Hotkey step: Idle(default prefilled) → Recording(capturing chord) → Bound(check) | Conflict(error, re-record)
```

- **Settings/Hub (Blush Playground)**: a left rail or stepper showing the four steps with check marks for
  completed ones; a single primary **charcoal** button (Next / Download / Finish) per step; Back as a
  secondary control. Empty/loading/error states per step as above (spinner while checking mic/GPU,
  progress bar while downloading, inline error cards with a retry affordance).
- Follow the Blush Playground accent discipline: **pumpkin (#EF724F)** is reserved for accent emphasis and
  focus rings (`focus-visible:ring-4 ring-pumpkin/45`), not button fills — primary buttons are charcoal.
  Keep ≥40px hit targets, visible focus rings, and never rely on color alone (mic-granted shows a check
  icon + label, not just green). Wizard is keyboard-navigable: Enter = Next, Esc = Back where
  non-destructive.

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
