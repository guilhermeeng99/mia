# Reuse from Toolzy

> **Status**: Applied (Phase 1 in progress) — the transcription registry / on-demand download / GPU-engine machinery described here has **landed** in [`app/src-tauri/src/stt.rs`](../app/src-tauri/src/stt.rs), and the `fetchExeWithDlls` pattern is realized in [`app/scripts/fetch-binaries.mjs`](../app/scripts/fetch-binaries.mjs) (whisper-server.exe + sibling DLLs). The one deviation from the original plan: the **MVP warm engine is the `whisper-server` sidecar** (cmake-free), not whisper-rs in-process — whisper-rs is the later optimization (ADR-004 revised). The "skip (live)" file-mode items remain deferred to the Phase 5 file feature.
> **Last updated**: 2026-05-29
> **Environment**: desktop (Windows, native)

MIA and **Toolzy** (`C:/Users/guiga/Documents/Projetos/Toolzy`) are sibling Tauri 2 + Rust + privacy-first desktop apps by the same owner. Toolzy already ships **local Whisper transcription in FILE mode** — pick a media file, preprocess with ffmpeg, run `whisper-cli`, write a transcript beside the input. MIA is the **live dictation** product: a global hotkey, a microphone, a warm model, and text typed at the cursor. The STT plumbing overlaps heavily, so MIA lifts Toolzy's transcription engine wholesale and adapts only the front of the pipe (file + ffmpeg → mic + VAD endpointing) and the model lifecycle (cold per-run spawn → **warm/resident** model).

This document is the precise reuse map: what comes over **as-is**, what gets **adapted**, and what is **skipped**. The source files are:

- `Toolzy/app/src-tauri/src/transcription.rs` — the engine (model registry, on-demand download, anti-hallucination arg builder, GPU engine, progress streaming, cancel).
- `Toolzy/app/scripts/fetch-binaries.mjs` — fetches the bundled native binaries (`fetchExeWithDlls` pattern for `whisper-cli.exe` + sibling DLLs).
- `Toolzy/docs/specs/transcription.md` — the spec these decisions are documented against.

Related MIA docs: [`docs/specs/speech-to-text.md`](specs/speech-to-text.md) (the adapted engine: warm model, models, GPU, VAD, anti-hallucination) and [`docs/specs/architecture.md`](specs/architecture.md) — in particular **ADR-004** (warm/resident STT) and **ADR-007** (on-demand model download + CUDA engine + fixed anti-hallucination defaults), which this reuse directly feeds.

---

## The headline adaptation

Toolzy's `transcribe_audio` is a **cold, file-based, per-run** pipeline:

```
file → ffmpeg (→ 16 kHz mono s16le WAV) → spawn whisper-cli (loads model) → write transcript → kill child
```

MIA's **live dictation** path keeps the recognition contract but replaces both ends:

```
hotkey → cpal mic (16 kHz mono PCM) → Silero VAD (silence gating) → WARM model (loaded once) → cleanup → inject at cursor
```

Two decisions are locked by the architecture:

1. **No ffmpeg preprocessing.** `cpal` already captures 16 kHz mono PCM, which is exactly what Whisper wants. The `ffmpeg`-sidecar `preprocess_to_wav` / `temp_wav_path` / `wav_stem` path is **not** on the live path — it is **skipped for v1**. (It is reusable verbatim by a future "transcribe a file" feature — see below.)
2. **Warm, resident model — not `whisper-cli` per utterance.** Live dictation cannot afford a multi-second model load on every push-to-talk. The model loads once and stays in RAM. MVP mechanism: **a warm `whisper-server` sidecar** (whisper.cpp's HTTP server) — loaded once at startup, each utterance is a localhost `/inference` POST of in-memory PCM; this is the cmake-free default that landed in `stt.rs`. **whisper-rs in-process** is the documented later optimization behind the same `SttBackend` seam. This is the **key divergence from Toolzy** (ADR-004 revised), and it means Toolzy's `whisper-cli` **sidecar spawn / `CommandChild` / `CommandEvent` event loop** is replaced, not reused, on the live path.

**Where the file path survives.** Toolzy's full cold pipeline — ffmpeg preprocessing **and** `whisper-cli` spawn — is exactly what a **"transcribe a file" feature (Phase 5 / Backlog)** needs. That future feature can reuse Toolzy's `transcribe_audio`, `preprocess_to_wav`, `run_ffmpeg`, `transcript_output_path`, and `format_ext` **verbatim**. So nothing is thrown away — the cold path is simply deferred, not deleted.

---

## 1. `transcription.rs` — item-by-item reuse map

Legend: **as-is** = copy with at most cosmetic renames (`toolzy-` → `mia-`); **adapt** = same idea, different inputs/lifecycle; **skip (live)** = not on the live dictation path (but reusable by the deferred file feature where noted).

| Item | Kind | Verdict | Notes for MIA |
|---|---|---|---|
| `HF_BASE`, `MODELS` (`ModelDef` registry), `WhisperModel` | const / struct | **adapt** | Keep the registry shape and HF base. Re-tune the default for dictation latency: Toolzy defaults to `large-v3` (max fidelity, slow on CPU); MIA's live default should favour responsiveness (likely `large-v3-turbo` or `medium` / `small`). Final defaults live in [`speech-to-text.md`](specs/speech-to-text.md). |
| `model_filename(id)` | pure fn | **as-is** | `ggml-<id>.bin`. Cargo-tested. |
| `model_url(id)` | pure fn | **as-is** | HF resolve URL. Cargo-tested. |
| `models_dir(app)` | fn | **as-is** | `app_data_dir()/models`. |
| `require_model(app, id)` | fn | **as-is** | Resolve-or-error before loading. MIA calls this when **warming** the model, not per utterance. |
| `download_file(url, dest, progress)` | fn | **as-is** | ureq stream → `.part` → rename on completion. Run inside `spawn_blocking`. The "no half model" guarantee carries over unchanged. |
| `VAD_FILENAME`, `VAD_URL` (`ggml-silero-v6.2.0.bin`, `ggml-org/whisper-vad`) | const | **as-is** | Silero VAD model. MIA uses Silero VAD for **two** jobs: (a) anti-hallucination gating inside Whisper, same as Toolzy, and (b) live **endpointing** of the mic stream (see [`docs/specs/audio-capture.md`](specs/audio-capture.md)). The downloaded `.bin` is shared. |
| `download_whisper_model(app, model, on_progress)` | `#[tauri::command]` | **as-is** | On-demand HF download + fetch VAD once + stream `DownloadProgress`. Drives MIA's **first-run model download gate** (see [`docs/specs/onboarding.md`](specs/onboarding.md)). |
| `list_whisper_models(app)` | `#[tauri::command]` | **as-is** | Lists models + `downloaded` flag for the picker. |
| `whisper_args(...)` anti-hallucination builder | pure fn | **adapt → landed** | The **anti-hallucination policy is reused exactly**; the *mechanism* changes because the warm engine is `whisper-server`, not the CLI. Toolzy's CLI flags `--temperature 0 --no-fallback --max-context 0` are **not** used (they are whisper-CLI flags). Instead `stt.rs` enforces the same guarantees per request: `inference_fields()` posts `temperature=0.0` + `temperature_inc=0.0` to `/inference` (zero `temperature_inc` disables whisper's temperature **fallback ladder** — the equivalent of `--no-fallback`), and each `/inference` call is **independent/stateless**, so there is no cross-utterance context conditioning (the equivalent of `--max-context 0`). Server startup (`server_args()`) only sets `-m`/`--host`/`--port`/`-t`. The `-f wav`, `-of out_base`, `-o<fmt>` (txt/srt/vtt) and `--print-progress` parts are **dropped** for live dictation (no file output, no progress bar — latency is sub-second). Cargo tests assert the deterministic `/inference` fields, retargeted at `inference_fields` instead of an argv builder. |
| `detected_language(stderr)` | pure fn | **adapt** | Useful when MIA runs auto-detect. The warm `whisper-server` returns the detected language in its `/inference` JSON reply rather than as stderr text, so the **intent** survives but the stderr string-parsing implementation does not carry over. (A future whisper-rs in-process build would expose it via API.) |
| `parse_whisper_progress(line)` | pure fn | **skip (live)** | `--print-progress` percentages are meaningless for a sub-second utterance. **Reusable as-is** by the deferred file-transcription feature. |
| `format_ext`, `transcript_output_path`, `wav_stem`, `temp_wav_path` | pure fn | **skip (live)** | File-output / temp-WAV machinery. No files on the live path. **Reusable as-is** by the deferred file feature. |
| `preprocess_to_wav(app, input, wav)` (`run_ffmpeg`) | fn | **skip (live)** | ffmpeg → 16 kHz mono WAV. Replaced by `cpal` capture, which already yields that format. **Reusable verbatim** by the deferred file feature. |
| `run_whisper(...)` (sidecar `.spawn()` + `CommandEvent` loop) | fn | **skip (live)** | Cold per-run `whisper-cli` spawn. Replaced by the **warm** whisper-rs in-process call (ADR-004). **Reusable** by the deferred file feature. |
| `drain_whisper(...)` | fn | **skip (live)** | Progress/stderr fan-out for the spawn loop. Tied to `run_whisper`. |
| `recognize(...)` (GPU-vs-CPU dispatch) | fn | **adapt** | The **dispatch idea** (use the installed CUDA engine when present, else the bundled CPU build) carries over — MIA warms the **CUDA `whisper-server` build** when `gpu_engine_status` says it is installed, else the bundled CPU build. The implementation differs because both branches are now warm (a resident server, not a per-run spawn). |
| `run_gpu_blocking(exe, args)` | fn | **adapt / skip (live)** | Plain `std::process::Command` with `CREATE_NO_WINDOW`. The `CREATE_NO_WINDOW` trick stays useful for spawning the warm `whisper-server` (CPU or CUDA build) so no console window flashes. (For a future whisper-rs in-process build this collapses into a linked call.) |
| **GPU engine block** — `GPU_URL`, `gpu_dir`, `gpu_exe`, `nvidia_present` (nvcuda.dll), `find_file`, `extract_zip` (bsdtar), `copy_engine_files`, `install_gpu_engine` | fn / const | **as-is** | The whole **detect → download → extract → place** flow for the self-contained NVIDIA CUDA build is reused unchanged (ADR-007). MIA points it at the warm engine's load path instead of a spawned exe; the acquisition machinery is identical. |
| `gpu_engine_status(app)` | `#[tauri::command]` | **as-is** | `{ gpuPresent (nvcuda.dll), downloaded }`. Drives MIA's "enable GPU engine" setting in [`docs/specs/settings.md`](specs/settings.md). |
| `download_gpu_engine(app, on_progress)` | `#[tauri::command]` | **as-is** | On-demand CUDA engine download, streamed. No-op when installed. |
| `TranscribeProgress { percent }` (Channel struct) | struct | **skip (live)** | Per-run progress bar. **Reusable** by the deferred file feature. The **Channel-for-streamed-progress pattern** itself is reused for **model/engine download** (`DownloadProgress`) — see below. |
| `DownloadProgress` (from `crate::download`) | struct / Channel | **as-is** | The download progress payload. Reused verbatim for MIA's model + CUDA-engine download gates. |
| `TranscribeState { child: Mutex<Option<CommandChild>> }` + `cancel_transcription` | struct / `#[tauri::command]` | **adapt** | Toolzy's "managed State holds the running child so Cancel can kill it" is reused as a **pattern**, but MIA's live model is warm (not a killable child). MIA's equivalent managed state holds the **warm session / capture handle**, and "cancel" means **stop the current utterance / discard the in-flight transcription** (e.g. release-to-cancel), not kill a process. See [`docs/specs/dictation.md`](specs/dictation.md). |
| `build_result(out, lang, stderr)` | fn | **skip (live)** | Reads the written transcript file back. No file on the live path; the transcript goes straight to cleanup + injection. **Reusable** by the deferred file feature. |
| `transcribe_audio(...)` | `#[tauri::command]` | **adapt → new command** | The **orchestrator is rewritten** for live dictation: no `path`/`format`/`task`, input is the mic stream, output is injected text. MIA's equivalent lives in [`docs/specs/dictation.md`](specs/dictation.md) (hotkey → capture → VAD → warm STT → cleanup → inject). The whole **`transcribe_audio` as written is reusable verbatim by the deferred file feature.** |

### Summary of `transcription.rs`

- **As-is (engine acquisition + registry + anti-hallucination policy):** `MODELS`/`model_url`/`model_filename`/`models_dir`/`require_model`, `download_file` (.part rename), `VAD_FILENAME`/`VAD_URL`, `list_whisper_models`, `download_whisper_model`, and the **entire GPU engine acquisition block** (`GPU_URL`, `nvidia_present`, `gpu_dir`/`gpu_exe`, `extract_zip`/`copy_engine_files`/`install_gpu_engine`, `gpu_engine_status`, `download_gpu_engine`, `find_file`), plus `DownloadProgress`.
- **Adapt:** `whisper_args` flag set → per-request `/inference` fields on the warm `whisper-server` (same anti-hallucination policy via `temperature=0` + `temperature_inc=0` + stateless calls), `recognize` GPU/CPU dispatch (now a warm CPU/CUDA server), `detected_language` (now from the `/inference` JSON), `TranscribeState`/`cancel_transcription` (warm session/capture handle instead of a killable child process).
- **Skip on the live path (but reusable verbatim by the deferred Phase 5 file-transcription feature):** ffmpeg `preprocess_to_wav`, `temp_wav_path`/`wav_stem`, `run_whisper`/`drain_whisper` (cold spawn), `parse_whisper_progress`/`TranscribeProgress`, `format_ext`/`transcript_output_path`/`build_result`, and `transcribe_audio` as a whole.

---

## 2. `fetch-binaries.mjs` — `fetchExeWithDlls` pattern

Toolzy bundles `whisper-cli.exe` plus its sibling `ggml`/`whisper` DLLs as a Tauri `externalBin` sidecar. The helper:

```js
async function fetchExeWithDlls({ url, member }, exeDest, label) { … }
```

downloads the whisper.cpp release zip (`whisper-bin-x64.zip`, MIT), extracts with `tar` (bsdtar ships on Windows 10+), finds `whisper-cli.exe`, copies it to the sidecar slot **and** copies every sibling `.dll` beside it (so the exe finds its libs both at dev time and in the bundle).

**Verdict: the `fetchExeWithDlls` pattern is reused — and has landed in [`app/scripts/fetch-binaries.mjs`](../app/scripts/fetch-binaries.mjs).** It finds **`whisper-server.exe`** (MIA's MVP warm engine) instead of `whisper-cli.exe`, copies it into `app/src-tauri/binaries/`, and copies every sibling `ggml`/`whisper` `.dll` beside it; `tauri.conf.json` gains `bundle.resources` so the installer ships them. Notes:

- MIA's MVP warm engine is the **`whisper-server` sidecar**, so the helper fetches **`whisper-server.exe`** plus its sibling `ggml`/`whisper` DLLs from the pinned whisper.cpp release. The server finds its libs both at dev time and in the bundle because the DLLs sit beside the exe. (A future **whisper-rs in-process** build would link/load the same whisper.cpp library family; the fetch helper would still be the way to obtain those DLLs.)
- **Windows x64 only** for v1 (ADR-011): MIA keeps only the `win32-x64` `TARGETS` entry; the macOS/Linux branches and the `linux`/`darwin` fallback prompts are dropped (deferred).
- Keep `findFile`, `download`, and `tar`-based extraction as-is.
- **Drop:** the `yt-dlp`, `ffmpeg`, `pdfium`, and `qpdf` fetches — see "Not reused" below. (MIA may keep the `ffmpeg` fetch **only** when/if the Phase 5 file-transcription feature lands.)

---

## 3. Patterns & conventions reused

These are project-wide habits MIA inherits from Toolzy (and which keep the owner's repos cohesive):

- **`Result<T, String>` IPC error model** (ADR-006) — every `#[tauri::command]` returns `Result<T, String>`; no panics across the Rust ↔ UI boundary. The whole `transcription.rs` follows this and MIA matches it.
- **Pure helpers + `#[cfg(test)]` cargo tests** — registries, URL/filename builders, and arg/param builders are pure and unit-tested (Toolzy tests `model_url`, `model_filename`, `whisper_args`, `parse_whisper_progress`, `find_file`, etc.). MIA keeps the same discipline for its arg/param builders, text-cleanup rules, and registries.
- **Tauri `Channel` for streamed progress** — Toolzy streams `DownloadProgress` and `TranscribeProgress` over a `Channel`. MIA reuses the Channel pattern for **model + CUDA-engine downloads** (and, on the future file feature, for transcription progress).
- **Bundle native binaries with the installer** — Toolzy ships native bits as Tauri `externalBin` sidecars. MIA bundles the warm `whisper-server.exe` + sibling DLLs via `tauri.conf.json` `bundle.resources` and spawns the server directly (`std::process::Command`, localhost-only `/inference`), so no Tauri `shell:allow-execute`/`shell:allow-spawn` capability is needed for it. (A future whisper-rs in-process build would need no spawned process at all — a further privacy/attack-surface win.)
- **On-demand "download gate" UX** — large models are not bundled; the app fetches the chosen model once (HF) and reuses it, with a clear one-time download prompt and a streamed progress bar. MIA's first-run flow ([`docs/specs/onboarding.md`](specs/onboarding.md)) and Hub settings ([`docs/specs/settings.md`](specs/settings.md)) reuse this gate, including the optional CUDA-engine gate.
- **`tauri-plugin-updater` signed auto-update** (ADR-009) — GitHub Releases + minisign-verified `latest.json` in-app update, lifted from Toolzy's distribution setup.
- **Documentation structure** — the whole `CLAUDE.md` + `docs/specs/` + `docs/specs/_template.md` + `docs/ROADMAP.md` layout, the status-block header convention, and the spec sections (Scope decisions locked / Business Rules / Engine Contract / Options & Defaults / Threading & Performance / UI States / Edge Cases / Testing Checklist / Out of Scope) are adopted directly from Toolzy.
- **Design-system token discipline** — Tailwind v4 utilities over raw hex, the proven Toolzy light-theme token set for the Settings/Hub surface, shared `components/ui/*`. See [`docs/specs/design-system.md`](specs/design-system.md). (MIA **adds** a new dark translucent HUD surface that Toolzy does not have.)

---

## 4. Not reused

- **Toolzy's non-audio tools** — image conversion, PDF (pdfium/qpdf), media editing, and `yt-dlp` download are irrelevant to a dictation app. Their `fetch-binaries.mjs` entries (`pdfium`, `qpdf`, `yt-dlp`) and Rust modules are dropped.
- **The file-only / no-microphone assumption** — Toolzy transcription is explicitly file-based and lists "live / streaming transcription and microphone capture" as **out of scope**. MIA inverts this: live mic capture (`cpal`) + VAD endpointing is the core. (The file mode itself is not discarded — it returns as a deferred Phase 5 feature that reuses Toolzy's cold path verbatim.)
- **The ffmpeg preprocessing step** — not needed for live capture (`cpal` already gives 16 kHz mono PCM). Reusable only by the deferred file feature.
- **The cold per-run `whisper-cli` spawn** — replaced by the warm/resident model (ADR-004), the latency-critical divergence from Toolzy.
- **React** — Toolzy's UI is React; MIA's UI is **Svelte 5** (runes) + Vite + Tailwind v4. The UI **components are not reused** (different framework); only the **token system and design discipline** carry over.
- **`whisper-cli` as the engine** — Toolzy chose the `whisper-cli` sidecar (cold per-run spawn, file in / file out). MIA's MVP uses a **warm `whisper-server` sidecar** instead — same whisper.cpp under the hood, but loaded once and driven per utterance over localhost `/inference` (no cmake build in CI, the latency-critical warm/resident divergence, ADR-004). **whisper-rs in-process** remains the documented later optimization behind the same `SttBackend` seam (accepting the C++/cmake build cost) if the server's overhead proves too high. `whisper-cli` itself survives only on the deferred Phase 5 file path.
