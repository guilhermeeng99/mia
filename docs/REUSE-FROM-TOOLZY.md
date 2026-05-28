# Reuse from Toolzy

> **Status**: Draft / Planned (Phase 0 — code does not exist yet)
> **Last updated**: 2026-05-28
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
hotkey → cpal mic (16 kHz mono PCM) → Silero VAD endpoint → WARM model (loaded once) → cleanup → inject at cursor
```

Two decisions are locked by the architecture:

1. **No ffmpeg preprocessing.** `cpal` already captures 16 kHz mono PCM, which is exactly what Whisper wants. The `ffmpeg`-sidecar `preprocess_to_wav` / `temp_wav_path` / `wav_stem` path is **not** on the live path — it is **skipped for v1**. (It is reusable verbatim by a future "transcribe a file" feature — see below.)
2. **Warm, resident model — not `whisper-cli` per utterance.** Live dictation cannot afford a multi-second model load on every push-to-talk. The model loads once and stays in RAM. Default mechanism: **whisper-rs in-process**; documented fallback: **whisper-server** (whisper.cpp's HTTP server). This is the **key divergence from Toolzy** (ADR-004), and it means the `whisper-cli` **sidecar spawn / `CommandChild` / `CommandEvent` event loop** is replaced, not reused, on the live path.

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
| `whisper_args(...)` anti-hallucination builder | pure fn | **adapt** | The **flag set is reused exactly** — `--vad --vad-model <silero> --temperature 0 --no-fallback --max-context 0` (ADR-007, fixed, not user-tunable). For the **warm** path these map to **whisper-rs `FullParams`** (greedy strategy, `temperature = 0.0`, `no_context = true`, VAD params) / whisper-server request params, **not** CLI argv. The `-f wav`, `-of out_base`, `-o<fmt>` (txt/srt/vtt) and `--print-progress` parts are **dropped** for live dictation (no file output, no progress bar — latency is sub-second). Keep the cargo test that asserts the anti-hallucination flags are always present, retargeted at the params struct. |
| `detected_language(stderr)` | pure fn | **adapt** | Useful when MIA runs auto-detect. whisper-rs exposes the detected language via API rather than stderr text, so the **intent** survives but the string-parsing implementation may not. Cargo-tested helper kept if the fallback whisper-server/CLI path is used. |
| `parse_whisper_progress(line)` | pure fn | **skip (live)** | `--print-progress` percentages are meaningless for a sub-second utterance. **Reusable as-is** by the deferred file-transcription feature. |
| `format_ext`, `transcript_output_path`, `wav_stem`, `temp_wav_path` | pure fn | **skip (live)** | File-output / temp-WAV machinery. No files on the live path. **Reusable as-is** by the deferred file feature. |
| `preprocess_to_wav(app, input, wav)` (`run_ffmpeg`) | fn | **skip (live)** | ffmpeg → 16 kHz mono WAV. Replaced by `cpal` capture, which already yields that format. **Reusable verbatim** by the deferred file feature. |
| `run_whisper(...)` (sidecar `.spawn()` + `CommandEvent` loop) | fn | **skip (live)** | Cold per-run `whisper-cli` spawn. Replaced by the **warm** whisper-rs in-process call (ADR-004). **Reusable** by the deferred file feature. |
| `drain_whisper(...)` | fn | **skip (live)** | Progress/stderr fan-out for the spawn loop. Tied to `run_whisper`. |
| `recognize(...)` (GPU-vs-CPU dispatch) | fn | **adapt** | The **dispatch idea** (use the installed CUDA engine when present, else the bundled CPU build) carries over — MIA warms the **CUDA whisper-rs / engine** when `gpu_engine_status` says it is installed, else the CPU build. The implementation differs because both branches are now in-process/warm, not spawned. |
| `run_gpu_blocking(exe, args)` | fn | **adapt / skip (live)** | Plain `std::process::Command` with `CREATE_NO_WINDOW`. For an in-process warm CUDA build this is replaced by linking the CUDA whisper-rs build; the `CREATE_NO_WINDOW` trick stays useful for any spawned fallback (whisper-server). |
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
- **Adapt:** `whisper_args` flag set → whisper-rs `FullParams` / whisper-server params (same anti-hallucination policy), `recognize` GPU/CPU dispatch (now warm in-process), `detected_language`, `TranscribeState`/`cancel_transcription` (warm session instead of child process).
- **Skip on the live path (but reusable verbatim by the deferred Phase 5 file-transcription feature):** ffmpeg `preprocess_to_wav`, `temp_wav_path`/`wav_stem`, `run_whisper`/`drain_whisper` (cold spawn), `parse_whisper_progress`/`TranscribeProgress`, `format_ext`/`transcript_output_path`/`build_result`, and `transcribe_audio` as a whole.

---

## 2. `fetch-binaries.mjs` — `fetchExeWithDlls` pattern

Toolzy bundles `whisper-cli.exe` plus its sibling `ggml`/`whisper` DLLs as a Tauri `externalBin` sidecar. The helper:

```js
async function fetchExeWithDlls({ url, member }, exeDest, label) { … }
```

downloads the whisper.cpp release zip (`whisper-bin-x64.zip`, MIT), extracts with `tar` (bsdtar ships on Windows 10+), finds `whisper-cli.exe`, copies it to the sidecar slot **and** copies every sibling `.dll` beside it (so the exe finds its libs both at dev time and in the bundle).

**Verdict: reuse the `fetchExeWithDlls` pattern as-is.** MIA needs the same native bits, with these notes:

- MIA's primary engine is **whisper-rs in-process**, which links/loads the whisper.cpp libraries rather than spawning `whisper-cli.exe`. The build still needs the **same DLLs** (`ggml`/`whisper`) available at runtime — `fetchExeWithDlls` is the proven way to fetch and place that family of binaries from the pinned whisper.cpp release. (Whether MIA links them via the crate's bundled build or fetches prebuilt DLLs is a build decision in [`docs/specs/speech-to-text.md`](specs/speech-to-text.md); the fetch helper is reused either way.)
- If MIA also ships the **whisper-server** fallback, that binary fetches with the **exact same helper** (download zip → find exe → copy exe + sibling DLLs).
- **Windows x64 only** for v1 (ADR-011): MIA keeps only the `win32-x64` `TARGETS` entry; the macOS/Linux branches and the `linux`/`darwin` fallback prompts are dropped (deferred).
- Keep `findFile`, `download`, and `tar`-based extraction as-is.
- **Drop:** the `yt-dlp`, `ffmpeg`, `pdfium`, and `qpdf` fetches — see "Not reused" below. (MIA may keep the `ffmpeg` fetch **only** when/if the Phase 5 file-transcription feature lands.)

---

## 3. Patterns & conventions reused

These are project-wide habits MIA inherits from Toolzy (and which keep the owner's repos cohesive):

- **`Result<T, String>` IPC error model** (ADR-006) — every `#[tauri::command]` returns `Result<T, String>`; no panics across the Rust ↔ UI boundary. The whole `transcription.rs` follows this and MIA matches it.
- **Pure helpers + `#[cfg(test)]` cargo tests** — registries, URL/filename builders, and arg/param builders are pure and unit-tested (Toolzy tests `model_url`, `model_filename`, `whisper_args`, `parse_whisper_progress`, `find_file`, etc.). MIA keeps the same discipline for its arg/param builders, text-cleanup rules, and registries.
- **Tauri `Channel` for streamed progress** — Toolzy streams `DownloadProgress` and `TranscribeProgress` over a `Channel`. MIA reuses the Channel pattern for **model + CUDA-engine downloads** (and, on the future file feature, for transcription progress).
- **`externalBin` + scoped shell capabilities** — bundle native binaries as sidecars with narrowly scoped `shell:allow-execute` / `shell:allow-spawn` capabilities. MIA reuses this for any spawned fallback (whisper-server). The in-process whisper-rs path needs no shell capability at all (a privacy/attack-surface win).
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
- **`whisper-cli` as the primary engine** — Toolzy chose the `whisper-cli` sidecar over `whisper-rs` (to avoid a C++/cmake build in CI and to get SRT/VTT for free). MIA makes the **opposite** choice for the live path — **whisper-rs in-process** for lowest latency — accepting the build cost because the warm model is non-negotiable for dictation. (whisper-server is the documented fallback; `whisper-cli` survives only on the deferred file path.)
