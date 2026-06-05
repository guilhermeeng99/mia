# Speech-to-Text Feature Spec

> **Status**: Phase 1 — engine implemented (whisper-server sidecar, ADR-004 revised default): model registry + on-demand download (progress `Channel`, `.part` rename), optional CUDA engine fetch, warm server lifecycle (spawn/wait/Drop), in-memory `transcribe_chunk`, and the `warm_status` command — all complete; pure helpers cargo-tested. whisper-rs in-process as a later optimization.
> **Last updated**: 2026-06-05
> **Coverage**: all sections drafted
> **Environment**: desktop (Windows, native)

The **STT engine** is the recognition stage of MIA's dictation pipeline (hotkey → capture → VAD → **STT** → cleanup → inject). It takes a finished utterance as 16 kHz mono PCM (handed off by [audio-capture.md](audio-capture.md) once Silero VAD has endpointed the speech) and returns a raw transcript, which the deterministic cleanup module ([text-cleanup.md](text-cleanup.md)) then polishes before injection ([text-injection.md](text-injection.md)). The whole-pipeline orchestration lives in [dictation.md](dictation.md); this spec owns only the engine: loading a Whisper model, keeping it **warm/resident**, and transcribing chunks with fixed anti-hallucination settings. It is the heart of **Phase 1 — Core Dictation MVP** (see [../ROADMAP.md](../ROADMAP.md)) and implements [ADR-003](architecture.md#adr-003-whisper-whispercpp-as-the-stt-engine) (Whisper for faithful pt-BR), [ADR-004](architecture.md#adr-004-warmresident-stt-for-live-dictation) (warm/resident model), and [ADR-007](architecture.md#adr-007-on-demand-model-download--cpu-bundled--optional-cuda-engine) (on-demand models + CUDA engine + fixed anti-hallucination defaults). The engine keeps the useful parts of Toolzy's file-transcription implementation — model registry, Hugging Face URLs, `.part` downloads, progress streaming, and CUDA engine acquisition — but adapts the runtime path to MIA's live dictation shape: cpal mic PCM into a warm `whisper-server`, not ffmpeg into a cold `whisper-cli`.

**Scope decisions** (locked at design time — 2026-05-28):

- **Engine — Whisper via whisper.cpp.** Chosen for faithful **Brazilian Portuguese**: Whisper is trained on broad multilingual data with strong pt-BR coverage, whereas NVIDIA's Parakeet/Canary are trained on **European** Portuguese (NVIDIA's own model cards note the pt-BR drop) and Parakeet is ASR-only. Whisper also covers ~99 languages, is MIT-licensed, and integrates as a clean self-contained engine ([ADR-003](architecture.md#adr-003-whisper-whispercpp-as-the-stt-engine)).
- **Warm/resident model — NOT cold per-utterance spawn.** Live dictation cannot pay a multi-second model load on every push-to-talk. The model loads **once** and stays in RAM. **MVP default mechanism: `whisper-server`** (whisper.cpp's HTTP server) spawned as a warm sidecar that MIA POSTs PCM to — chosen because it is **cmake-free** (a prebuilt binary, fetched via Toolzy's pattern) and builds out of the box, while still loading the model only once. **Later optimization: `whisper-rs` in-process** (lowest latency, no IPC/localhost hop, no shell capability), deferred because it builds whisper.cpp via cmake. Both sit behind one `SttBackend` trait. Either way this is the **key divergence from Toolzy**, which cold-spawns `whisper-cli` per run ([ADR-004](architecture.md#adr-004-warmresident-stt-for-live-dictation)). _(Revised 2026-05-28 — the build-toolchain audit found cmake absent; see ADR-004's revision note.)_
- **No ffmpeg, no temp WAV, no file output on the live path.** `cpal` already captures exactly the 16 kHz mono PCM Whisper wants, so the old file-transcription machinery (`preprocess_to_wav`, temp paths, output transcript path, per-run result builder) is **skipped** on live dictation. Those pieces remain a useful shape for a deferred Phase 5 file-transcription feature, but the live path sends the transcript straight to cleanup + injection, never to disk; audio stays in memory ([ADR-001](architecture.md#adr-001-native-on-device-privacy-first)).
- **Model choice favours latency (dictation), not max fidelity (Toolzy).** Toolzy defaults to `large-v3` (slowest, most faithful) because correctness-over-speed is its goal. **MIA's live default favours responsiveness** — `small` (CPU) / `medium` as the recommended balance, with `large-v3-turbo` and `large-v3` selectable for users who want more accuracy and can afford the latency or have the CUDA engine. A two-second wait is fine for a one-off file transcription; it is unacceptable when typing at the cursor. See §4 and rule 6.
- **Anti-hallucination defaults are fixed, always on, never user-tunable.** Every recognition runs with **Silero VAD** + **greedy decoding (temperature 0)** + **no temperature fallback ladder** + **no cross-utterance context conditioning**. With the MVP **whisper-server** backend these are enforced **per request** on `/inference`: `temperature=0.0` **and** `temperature_inc=0.0` (which disables whisper's temperature-fallback ladder — the equivalent of whisper-CLI's `--no-fallback`), and each `/inference` call is **independent/stateless**, so no previous transcript conditions the next (the equivalent of whisper-CLI's `--max-context 0`). Those two literal CLI flags are *not* passed (and are not needed) with whisper-server; the in-process whisper-rs path maps the same policy to `FullParams` (greedy, temperature 0, `no_context = true`). These prevent Whisper's known failure mode (inventing/looping text over silence or between utterances) and are pinned engine settings, not knobs ([ADR-007](architecture.md#adr-007-on-demand-model-download--cpu-bundled--optional-cuda-engine), rules 7–8).
- **Models download on demand; CPU bundled, CUDA optional.** Whisper ggml models are 466 MB–3 GB, so they are fetched once from Hugging Face (`ggerganov/whisper.cpp`) to app-data and reused, behind a clear **download gate**. The small CPU whisper.cpp build is bundled (works everywhere); on NVIDIA the user can one-click download a self-contained **CUDA** engine (~7–10× faster). Lifts Toolzy's download + GPU-engine machinery wholesale ([ADR-007](architecture.md#adr-007-on-demand-model-download--cpu-bundled--optional-cuda-engine)).

---

## 1. Inputs / Outputs

MIA is a live dictation app, so the engine is framed around **audio in → text out**, not file formats.

| Aspect | This feature |
|---|---|
| **Trigger** | An endpointed utterance handed off from [audio-capture.md](audio-capture.md) (VAD detected speech-end, or push-to-talk released — see [hotkeys.md](hotkeys.md)). The engine itself exposes in-process `warm_model` / `transcribe_chunk` / `unload` helpers; it does not own the hotkey. |
| **Audio in** | 16 kHz **mono** PCM `f32` samples (cpal capture), already in Whisper's required format — **no ffmpeg, no resample, no temp WAV**. One utterance's worth of samples per `transcribe_chunk`. |
| **Text in** | N/A (the transcript is produced here). |
| **Text out** | Raw UTF-8 transcript string + detected/forced language code, returned to the dictation orchestrator → cleanup → injection. Never written to disk. |
| **Target** | The dictation pipeline (in-process). Not a window — injection is a separate stage. |
| **Language** | Auto-detect (default) or forced; **pt-BR** and **English** first-class; ~99 Whisper languages supported. |

Engine: **whisper.cpp** via a warm **whisper-server** sidecar (MVP default, cmake-free) or **whisper-rs** in-process (later optimization); **Silero VAD** (`ggml-silero-v6.2.0.bin`) gates silence inside Whisper *and* is reused for live endpointing in [audio-capture.md](audio-capture.md) — the same downloaded `.bin`. The audio buffer **never touches disk** (ADR-001): samples flow from the cpal ring buffer straight into the warm model.

---

## 2. Engine Contract (Rust)

Rust is the engine; the Svelte UI is a thin webview that only calls typed `invoke()` wrappers (see [architecture.md](architecture.md)). All commands return `Result<T, String>` — no panics across the IPC boundary ([ADR-006](architecture.md#adr-006-resulttstring-error-model-across-the-ipc-boundary)).

**Module**: `app/src-tauri/src/stt.rs` (adapted from Toolzy's `transcription.rs`).

The warm model lives in managed Tauri `State` so it is loaded once and shared. The latency-critical `transcribe_chunk` is **not** a `#[tauri::command]` on the hot path in v1 — it is called **in-process by the dictation orchestrator** ([dictation.md](dictation.md)) so an utterance never round-trips through the webview. In the **MVP default** (`whisper-server` backend) `transcribe_chunk` POSTs the PCM to the warm local server on `127.0.0.1` and parses the JSON reply; the **in-process optimization** (`whisper-rs`) later removes even that localhost hop. Both are hidden behind the `SttBackend` trait, so the orchestrator code is identical. The commands below are the lifecycle/management surface the UI drives (warm-up at onboarding, model picker, GPU gate).

```rust
// app/src-tauri/src/stt.rs

#[derive(serde::Serialize)]            // rename_all = "camelCase"
struct WhisperModel { id: String, label: String, size_mb: u32, downloaded: bool, recommended: bool }

#[derive(serde::Serialize)]
struct GpuStatus { gpu_present: bool, downloaded: bool } // nvcuda.dll present; CUDA engine installed

#[derive(serde::Serialize)]
struct WarmStatus { loaded: bool, model: Option<String>, backend: String } // "whisperRs" | "whisperServer"; gpu flag

#[derive(serde::Serialize)]
struct Transcript { text: String, language: String } // raw transcript + detected/forced code

/// Warm/resident state: the warm whisper-server child (MVP default) — or, later, the
/// in-process whisper-rs context — plus which model/backend is live. Lives in managed
/// Tauri State; loaded ONCE.
struct SttState { /* Mutex<Option<WarmModel>>, current model id, backend, gpu flag */ }

// ---- lifecycle (in-process helpers, NOT #[tauri::command]; called by the orchestrator/settings) ----

/// Load the chosen model into RAM and keep it resident (warm whisper-server sidecar by
/// default — MVP, cmake-free; in-process whisper-rs is the later optimization). Idempotent:
/// a no-op if the same model+backend is already warm; reloads if the model/backend changed.
/// Err if the model isn't downloaded (require_model) — callers gate with download_whisper_model first.
/// NOT exposed to the UI: an in-process fn called by the dictation orchestrator / settings.
pub fn warm_model(app: &AppHandle, state: &SttState, model: &str) -> Result<(), String>;

/// Whether a model is currently warm, which one, and on which backend/engine.
#[tauri::command]
fn warm_status(state: State<'_, SttState>) -> Result<WarmStatus, String>;

/// Free the resident model (RAM reclaim; e.g. update_settings releasing the model).
/// In-process helper (NOT a command); also runs on app exit via Drop.
pub fn unload(state: &SttState) -> Result<(), String>;

// ---- recognition (called IN-PROCESS by dictation.rs on the hot path; see dictation.md) ----

/// Transcribe one endpointed utterance against the WARM model. Pure recognition:
/// no cleanup, no injection. `language` None ⇒ auto-detect, Some("pt") forces.
/// Always applies the fixed anti-hallucination policy (Silero VAD + greedy temp 0 +
/// temperature_inc 0 so no fallback ladder + stateless per-request = no cross-utterance
/// context). Err if no model is warm.
fn transcribe_chunk(state: &SttState, samples: &[f32], language: Option<&str>) -> Result<Transcript, String>;

// ---- model + engine acquisition (reused AS-IS from Toolzy transcription.rs) ----

#[tauri::command]
fn list_whisper_models(app: AppHandle) -> Result<Vec<WhisperModel>, String>;
#[tauri::command]
async fn download_whisper_model(app: AppHandle, model: String, on_progress: Channel<DownloadProgress>) -> Result<String, String>;
#[tauri::command]
fn gpu_engine_status(app: AppHandle) -> Result<GpuStatus, String>;
#[tauri::command]
async fn download_gpu_engine(app: AppHandle, stt: State<'_, SttState>, on_progress: Channel<DownloadProgress>) -> Result<(), String>; // stt: hot-swap the warm engine to CUDA after install
```

**Pure helpers** (`#[cfg(test)]` cargo-tested, no I/O — lifted from Toolzy):

```rust
fn model_filename(id: &str) -> Option<String>;   // "ggml-<id>.bin"        (as-is)
fn model_url(id: &str) -> Option<String>;         // HF resolve URL         (as-is)
fn nvidia_present() -> bool;                       // SystemRoot\System32\nvcuda.dll exists (as-is)
fn find_file(dir: &Path, name: &str) -> Option<PathBuf>; // locate exe/DLL in extracted CUDA zip (as-is)

/// The anti-hallucination policy, adapted from Toolzy's CLI `whisper_args`. With the MVP
/// whisper-server backend it is expressed as per-request `/inference` form fields —
/// `inference_fields(language)` emits `temperature=0.0` + `temperature_inc=0.0` (disables the
/// fallback ladder) + `response_format=json` (+ optional `language`); each call is stateless
/// (no cross-utterance context), and `server_args` starts the warm server. cargo-tested to
/// assert every anti-hallucination setting is present regardless of language/model. The later
/// in-process whisper-rs path maps the same policy to `FullParams` (greedy, temperature 0,
/// `no_context = true`, VAD with the Silero model).
fn inference_fields(language: Option<&str>, prompt: Option<&str>) -> Vec<(String, String)>; // anti-hallucination + dictionary bias prompt (whisper-server)
fn server_args(model: &Path, vad_model: &Path, port: u16, threads: usize) -> Vec<String>; // warm-server startup args (incl. Silero --vad-model)
```

- **Backend selection**: the warm **whisper-server** sidecar is the **MVP default** (cmake-free); MIA spawns it once and POSTs PCM to `127.0.0.1` per utterance. **whisper-rs in-process** is the **later optimization** (no IPC/localhost hop, no shell capability), deferred because it builds whisper.cpp via cmake; the same anti-hallucination params map to either backend. The CUDA engine is selected at warm time when `gpu_engine_status` reports it installed (the warm server/engine loads the CUDA build instead of the CPU build) — the dispatch *idea* is Toolzy's `recognize`, but the model is loaded **once** and kept warm, not cold-spawned per run.
- **Native bits**: the **`whisper-server.exe`** binary plus its sibling whisper.cpp `ggml`/`whisper` DLLs must be present at runtime; they are fetched/placed into `app/src-tauri/binaries/` by `app/scripts/fetch-binaries.mjs` (Toolzy's `fetchExeWithDlls` pattern) and shipped via `tauri.conf.json` `bundle.resources` (Windows x64 only — [ADR-011](architecture.md#adr-011-windows-only-v1)). The whisper-server backend uses a scoped `shell:allow-spawn`; the later in-process whisper-rs path needs **no shell capability** (privacy/attack-surface win).
- **Error messages** (each maps to a UI/HUD state): `"unknown model: <id>"`, `"model not downloaded: <id>"`, `"model not downloaded: silero VAD"`, `"no model loaded"` (transcribe before warm), `"download failed: …"`, `"whisper engine failed: <reason>"`.
- **Provenance**: `list_whisper_models`, `download_whisper_model`, `download_file` (`.part` rename + SHA-256 verification), `VAD_FILENAME`/`VAD_URL`, `gpu_engine_status`, `download_gpu_engine`, and the CUDA detect→download→extract→place block are the parts adapted from Toolzy's file transcription engine. The live-only pieces (`warm_model`, `transcribe_chunk`, capture integration, HUD orchestration) are MIA-specific.
- **UI wrapper**: `app/src/lib/stt.ts` (`warmModel`, `warmStatus`, `unloadModel`, `listWhisperModels`, `downloadWhisperModel`, `gpuEngineStatus`, `downloadGpuEngine`) — one typed `invoke()` per command. The UI holds **no** recognition logic.

---

## 3. Business Rules

Numbered, testable.

1. **Warm once, reuse** — `warm_model` loads the model into RAM and keeps it resident. A subsequent `warm_model` for the **same** model+backend is a no-op (already warm); a **different** model/backend reloads. `transcribe_chunk` reuses the resident model and never reloads it ([ADR-004](architecture.md#adr-004-warmresident-stt-for-live-dictation)).
2. **No transcription without a warm model** — `transcribe_chunk` returns `Err("no model loaded")` if nothing is warm (the orchestrator warms during onboarding / first hotkey press).
3. **Model required on disk** — `warm_model` checks `models/ggml-<id>.bin` exists (`require_model`); if not → `Err("model not downloaded: <id>")` **before** loading. `model_url`/`model_filename` return `None` for an unknown id → `Err("unknown model: <id>")`. The Silero VAD model is checked too → `Err("model not downloaded: silero VAD")`.
4. **Input format** — `transcribe_chunk` expects 16 kHz **mono** `f32` PCM (cpal native output). No resampling/ffmpeg on the live path; a malformed buffer is a programming error, not a user path.
5. **Language** — `None` ⇒ auto-detect (Whisper picks); `Some(code)` forces it (e.g. `pt`, `en`). The detected/forced code is returned in `Transcript.language`. pt-BR and English are first-class; ~99 codes accepted (rule 9).
6. **Latency-favouring default model** — the recommended live default is `small`/`medium`, **not** Toolzy's `large-v3`. `large-v3-turbo` / `large-v3` are selectable for accuracy at a latency cost. The engine never silently upgrades the model (rule 1).
7. **Silence gating (anti-hallucination, fixed)** — recognition **always** runs with **Silero VAD**, so only detected speech reaches Whisper; silence / non-speech cannot become invented text. Not user-tunable ([ADR-007](architecture.md#adr-007-on-demand-model-download--cpu-bundled--optional-cuda-engine)).
8. **Deterministic decoding (anti-hallucination, fixed)** — recognition **always** uses greedy decoding (temperature 0), **no temperature fallback ladder**, and **no previous-text conditioning** to stop repetition/looping drift between utterances. With whisper-server this is `temperature=0.0` + `temperature_inc=0.0` per `/inference` (the fallback-ladder kill) plus the fact that each `/inference` call is stateless (no cross-utterance context) — i.e. the same effect as whisper-CLI's `--no-fallback` / `--max-context 0`, but achieved via the server's request fields rather than those literal flags. `inference_fields` always emits them; cargo-tested.
9. **Empty / no-speech utterance** — if VAD finds no speech, recognition returns an **empty** string (`Ok`, never hallucinated text); the orchestrator injects nothing and returns the HUD to Idle.
10. **GPU when available** — if `gpu_engine_status.downloaded` is true (and `gpu_present`), `warm_model` loads the **CUDA** engine/build (≈7–10× faster); otherwise the bundled **CPU** build. No NVIDIA ⇒ CPU, transparently.
11. **Download integrity** — `download_whisper_model`, `download_gpu_engine`, and the release-time CPU binary fetch stream to a `.part` file, verify a pinned SHA-256, and rename only after the hash matches. An interrupted or tampered download leaves **no** trusted model/engine in place. Progress is reported via the `Channel` (reused from Toolzy `download_file`). `list_whisper_models` uses an exact byte-size check for the UI so the Hub opens without hashing multi-GB files; the full SHA-256 check still runs before download reuse, warm-up, and use.
12. **Unload frees RAM** — `unload` drops the resident context; subsequent `transcribe_chunk` → `Err("no model loaded")` until re-warmed.

---

## 4. Options & Defaults

Every user-facing parameter; the anti-hallucination settings are **fixed**, not options.

| Option | Type | Range / values | Default | Effect |
|---|---|---|---|---|
| Model | enum id | see registry below | `small` (live) | accuracy ↔ latency/RAM trade-off; warmed once |
| Language | code or auto | `auto` + popular pinned codes in the UI (`pt`, `en`, `es`, `fr`, `de`, `it`, `nl`, `pl`, `ru`, `uk`, `tr`, `ar`, `hi`, `id`, `ja`, `ko`, `zh`); Whisper can still auto-detect broad multilingual input | `auto` | force vs detect source language; independent from the UI locale |
| Backend | enum | `whisperServer` · `whisperRs` | `whisperServer` | warm sidecar (MVP default, cmake-free) vs in-process (later optimization); advanced/diagnostic |
| GPU engine | bool (gated) | on if installed + NVIDIA | off until downloaded | use CUDA build when present (≈7–10× faster) |

**Model registry** (ggml, fetched from `huggingface.co/ggerganov/whisper.cpp` — same registry shape as Toolzy, re-tuned default):

| id | label | ~size | notes |
|---|---|---|---|
| `small` | Small | ~466 MB | **live default** — fast, low latency, good pt-BR for dictation |
| `medium` | Medium | ~1.5 GB | balanced; recommended if CPU has headroom |
| `large-v3-turbo` | Large v3 Turbo | ~1.6 GB | more accurate, still reasonable latency (esp. with CUDA) |
| `large-v3` | Large v3 | ~3.1 GB | most faithful (max fidelity), slowest on CPU; best with the CUDA engine |

> **Why the default differs from Toolzy.** Toolzy is a one-shot file transcriber where "correctness over speed" rules, so it defaults to `large-v3` and a 20-second run is fine. MIA types at the cursor in real time — the dominant cost is STT inference on the utterance, and a `large-v3` CPU pass would add seconds of perceptible lag after every phrase. So MIA inverts the default to a small/medium model (latency-first), and lets users opt into more accuracy (or pair a larger model with the CUDA engine). The **anti-hallucination policy is identical** in both apps; only the speed/accuracy default moves.

**Fixed (not user options)** — Silero VAD on, greedy/temperature 0, no temperature fallback ladder (whisper-server `temperature_inc=0.0`), and stateless per-utterance recognition (no cross-utterance context) — the equivalents of whisper-CLI's `--no-fallback` / `--max-context 0`, but enforced via the server's request fields, not those literal flags. A correct result never depends on the user knowing to enable them ([ADR-007](architecture.md#adr-007-on-demand-model-download--cpu-bundled--optional-cuda-engine), rules 7–8).

The UI (onboarding + Hub settings) offers the model picker with each model's **size**, fidelity/latency trade-off, and a **download gate**: if the chosen model (or the Silero VAD model) isn't on disk, the user must download it once before selecting it. Downloading a model does not silently change the active model; a downloaded model becomes selectable, and the selected model applies to the next dictation. The engine re-checks existence defensively (rule 3).

---

## 5. Threading / Performance

Live dictation is latency-critical — the warm-model contract is the whole point of this spec.

- **Audio thread**: the cpal capture callback runs on its own real-time thread and only fills a ring buffer ([audio-capture.md](audio-capture.md)); **no STT in the callback**. An utterance's samples are handed to `transcribe_chunk` off that thread.
- **Warm model**: the model is loaded **once** by `warm_model` (during onboarding or on first hotkey arm) and kept resident in `SttState` ([ADR-004](architecture.md#adr-004-warmresident-stt-for-live-dictation)). This feature **does not** cold-spawn `whisper-cli` per utterance (the Toolzy behaviour); that eliminates the multi-second model-load tax on every push-to-talk.
- **Recognition off the UI/audio threads**: `transcribe_chunk` runs on a worker (spawned/blocking task), never on the cpal callback and never blocking the webview. The result is handed back to the dictation orchestrator, then to cleanup + injection.
- **Latency budget**: utterance-end → first injected char. The dominant cost is **STT inference** on the warm model; model load (the expensive part) is already paid. Everything else — VAD endpointing, deterministic cleanup, SendInject — is off the critical inference path and sub-100 ms. The model choice (rule 6 / §4) is the main lever; the CUDA engine (rule 10) is the other (≈7–10×).
- **Cancellation**: a release-to-cancel or timeout discards the in-flight utterance — the warm model is **not** killed (it's warm, not a child process), the in-flight `transcribe_chunk` result is dropped, and the HUD returns to Idle. This adapts Toolzy's `TranscribeState` "cancel" (which killed a child) into "discard the current utterance against the still-warm model" — see [dictation.md](dictation.md).
- **Resource use**: model RAM is the loaded ggml size (≈0.5–3 GB depending on model; CPU vs CUDA build). Models and the CUDA engine are **lazy** — fetched only on the download gate, never bundled (CPU whisper.cpp build is the only bundled engine). One model is warm at a time; `unload` reclaims its RAM.

---

## 6. UI States

The STT engine has **two** surfaces. On the hot path it drives the **floating mic HUD** (white Blush pill, always-on-top) only for its **transcribing** phase; the surrounding listening/inserting states belong to the wider pipeline ([dictation.md](dictation.md), [tray-and-hud.md](tray-and-hud.md)). Its **lifecycle/management** (warm-up status, model picker, download/GPU gates) lives in the **Settings/Hub window** (Blush Playground) and **onboarding** ([settings.md](settings.md), [onboarding.md](onboarding.md), [design-system.md](design-system.md)).

```
HUD (live, this feature's slice):
  Listening(pulsing waveform) → Transcribing(spinner ← transcribe_chunk running)
        → Inserting(brief check) | Error(message)

Hub/onboarding (model lifecycle):
  Not downloaded → Downloading(progress) → Downloaded
        → Selected → Warming(spinner) → Warm(ready) | Error
  (+ optional) GPU: Not installed → Downloading → Installed
```

- **HUD** (while dictating): the **Transcribing** state shows the single pumpkin spinner over the white Blush pill (white, 2px charcoal outline) while `transcribe_chunk` runs; an engine error surfaces as the HUD **Error** state (not just a log). Keep the one-action-color discipline; click-through where possible.
- **Hub/onboarding**: model picker with each model's **size**; a clear **download gate** when missing (one-time "download once" prompt + streamed MB progress bar); a **warm-up** indicator (Warming → Warm/ready); and the optional **GPU engine** toggle gated on `gpu_engine_status` (NVIDIA present + installed). Empty/loading/error states per the design system.
- ≥40px hit targets; never rely on color alone (pair the listening accent with the waveform motion and the spinner with text).

---

## 7. Edge Cases

| Scenario | Expected behavior |
|---|---|
| Chosen model not downloaded | `Err("model not downloaded: <id>")` (no load); UI/onboarding shows the download gate ([onboarding.md](onboarding.md)) |
| Silero VAD model missing | `Err("model not downloaded: silero VAD")`; downloaded once on the model gate (reused from Toolzy) |
| Unknown model id | `Err("unknown model: <id>")` (no load) |
| `transcribe_chunk` before warm | `Err("no model loaded")`; orchestrator warms first |
| Silence / VAD detects no speech | empty transcript (`Ok`), no injection, HUD → Idle — never hallucinated text (rule 9) |
| Download interrupted (offline/cancel) | `.part` discarded; `Err("download failed: …")`; no partial model/engine (rule 11) |
| NVIDIA driver absent | CUDA engine never offered (`gpu_present=false`); CPU build serves, transparently |
| CUDA engine installed but model is `large-v3` on a weak GPU | still runs; latency is the user's chosen trade-off (rule 6) |
| warm `whisper-server` fails to spawn / not reachable | surface `Err("whisper engine failed: …")`; if the user opted into `whisper-rs` and it fails to load, fall back to spawning `whisper-server` |
| Hotkey released mid-transcription | in-flight `transcribe_chunk` result discarded; warm model untouched; never inject stale text (§5, [dictation.md](dictation.md)) |
| Re-`warm_model` same model | no-op (already warm, rule 1) |

---

## 8. Testing Checklist

- **Rust** (`cargo test`, no I/O — pure helpers only):
  - [ ] `model_url` / `model_filename` — known ids map to the HF URL + `ggml-<id>.bin`; unknown → `None` (lifted Toolzy tests).
  - [ ] `inference_fields` — **always** emits `temperature=0.0` + `temperature_inc=0.0` (fallback-ladder kill) (+ optional forced `language`), regardless of language/model; Silero VAD on and stateless per-utterance recognition supply the rest of the policy (rules 7–8).
  - [ ] `nvidia_present` / `find_file` — driver detection + locating the CUDA exe/DLLs in an extracted archive (lifted Toolzy tests).
  - [ ] each `Err(String)` path: unknown model, model-not-downloaded, VAD-not-downloaded, no-model-loaded.
  - [ ] warm-state transitions where pure: warm → re-warm same model is a no-op; warm different model reloads; `unload` → not-loaded.
- **Manual / runtime** (needs mic, a downloaded model, and a real focused app):
  - [ ] happy path: hotkey → speak → text appears at cursor (pt-BR **and** English).
  - [ ] warm model: second utterance shows **no** model-reload delay (warm, not cold).
  - [ ] HUD reflects the **Transcribing** state (spinner) and surfaces engine errors.
  - [ ] auto-detect vs forced language both return the right `Transcript.language`.
  - [ ] model download gate shows progress; interrupting leaves no partial model.
  - [ ] CUDA engine: with NVIDIA + engine installed, a `large-v3-turbo`/`large-v3` utterance is markedly faster than CPU; without NVIDIA, CPU serves transparently.
  - [ ] silence / no-speech press → nothing injected (no hallucinated text).

---

## 9. Out of Scope (this version)

- **Streaming live partials** (text appearing word-by-word as you speak) — v1 transcribes a whole endpointed utterance; partial-as-you-speak streaming is **Phase 5 / Backlog** ([../ROADMAP.md](../ROADMAP.md)).
- **GPU keep-warm sub-second latency tuning** — the CUDA engine is shipped, but squeezing sub-second end-to-end via persistent GPU context is a backlog optimization.
- **File / batch transcription** — recognizing an existing media file (with ffmpeg preprocessing, file output, and a per-run progress bar) is a deferred **Phase 5** feature. It would reuse the old cold-file shape (`preprocess_to_wav`, `run_whisper`, `transcript_output_path`, `transcribe_audio`) while sharing MIA's current model registry, download integrity checks, and anti-hallucination policy. Not on the live path.
- **Translation into Portuguese / non-English targets** — Whisper's `translate` task is English-only; faithful X→pt needs a separate MT engine (license-incompatible options aside). Dictation transcribes the spoken language; it does not translate.
- **Non-NVIDIA GPU** (AMD/Intel via Vulkan) — only the NVIDIA CUDA engine is offered; CPU serves everyone else.
- **Speaker diarization** ("who spoke when") — single-speaker dictation only.
- **macOS / Linux engine builds** — Windows x64 only for v1 ([ADR-011](architecture.md#adr-011-windows-only-v1)); deferred.
