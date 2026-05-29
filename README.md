# MIA

**Your voice, your machine. Local dictation for Windows.**

Open-source, local, privacy-first voice dictation for Windows — an offline alternative to [Wispr Flow](https://wisprflow.ai).

> **Status**: Phase 1 (Core Dictation MVP) — **core loop validated on Windows end-to-end.** The full live loop runs on a real machine: global PTT → cpal capture → **server-side Silero-VAD-gated** warm whisper-server → deterministic cleanup → SendInput injection, in pt-BR + English, with the floating HUD reflecting each phase. Phase 0 (Docs & Design) is complete. Phase 2 (AI Command Mode / Polish) is **descoped** — MIA stays a faithful dictation tool. Phase 4 **signed in-app auto-update** and the optional **NVIDIA CUDA** engine are **shipped** (CUDA validated on an RTX 4050); the remaining Phase 4 work is cutting the first signed `v*` release. See [docs/ROADMAP.md](docs/ROADMAP.md) for per-feature status.
> **Last updated**: 2026-05-29
> **Environment**: desktop (Windows, native)

Press a global hotkey (push-to-talk), speak, and MIA types polished text at your cursor — in whatever app is focused. Everything runs on your machine. No cloud, no account, no server. Your voice never leaves the device.

---

## Why MIA

- **Local & private by design.** Audio is captured, transcribed, cleaned, and injected entirely on your machine. No network round-trips, no telemetry of your speech, no account to create.
- **Free and open source (MIT).** Permissive dependencies only — never anything copyleft-viral bundled in.
- **An offline answer to Wispr Flow.** Same push-to-talk-and-it-types ergonomics, but fully on-device. Wispr Flow is cloud-based; MIA is not.
- **pt-BR is first-class.** MIA uses Whisper (whisper.cpp) specifically for its faithful Brazilian Portuguese transcription. English is first-class too, and Whisper covers ~99 languages.

---

## Features

Honest status legend: ✅ Done · 🚧 In progress · ⬜ Planned · 💡 Backlog. Phase 1 is underway — the markers below mirror [docs/ROADMAP.md](docs/ROADMAP.md).

### Core dictation (Phase 1 — 🚧 In progress)
- ✅ End-to-end live loop **validated on Windows**: global PTT → cpal capture → server-side VAD-gated warm whisper-server → deterministic cleanup → SendInput injection at the cursor.
- ✅ Global **push-to-talk** hotkey that works even when MIA is unfocused (default Ctrl+Space) — runtime registration, press/hold + toggle reducer, chord recorder, and conflict-probe all wired.
- ✅ Live microphone capture (16 kHz mono) with device selection and a live level meter streamed to the HUD.
- ✅ **Silero VAD** gating — recognition is **VAD-gated server-side** (the warm whisper-server runs with `--vad --vad-model <silero>`), so only detected speech reaches the decoder.
- ✅ **Warm/resident Whisper** model (loaded once via a warm **whisper-server** sidecar, not respawned per utterance) for low latency.
- **Anti-hallucination, always on**: Silero VAD + greedy decoding (whisper-server `/inference` at temperature 0 with the temperature-fallback ladder disabled) + stateless, independent per-utterance requests (no previous-text conditioning across utterances).
- ✅ **Deterministic text cleanup**: filler-word removal (um/uh/é/tipo/né…), spoken-punctuation substitution ("nova linha", "ponto", "vírgula"), stutter/repeat collapse, whitespace normalization, sentence casing.
- ✅ **System-wide text injection** at the cursor via Windows `SendInput` (Unicode keystrokes), with a clipboard + Ctrl+V fallback for long text.
- ✅ System **tray** icon (Open / Quit) and a floating **mic HUD** overlay in its own transparent, always-on-top, click-through window (listening → transcribing → inserting, with a live waveform).
- ✅ **On-demand model download** gate with streamed progress (models fetched from Hugging Face).
- ✅ **pt-BR + English** first-class language selection (Automático / Português (pt-BR) / English), forwarded per utterance to the warm Whisper path.
- 🚧 Genuinely remaining (runtime): toggle-mode auto-endpoint (client-side per-frame Silero) and elevated-window (UIPI) injection.

### AI magic (Phase 2 — ❌ Descoped)
**Dropped by product decision (2026-05-29):** MIA stays a faithful, deterministic dictation tool — the local-LLM Command Mode / Polish layer is **not** wanted. A runtime (warm `llama-server` + GGUF download) was built and then **reverted**; only the pure, cargo-tested helpers remain dormant in `ai_commands.rs`, wired to nothing. If AI is ever reconsidered, that core is the starting point.

### Personalization (Phase 3 — ⬜ Planned)
- 🚧 Custom dictionary / personal vocabulary and word replacement (pure core + CRUD commands + Hub section done; bias-prompt wiring remains).
- 🚧 Voice-triggered snippets (text expansion) (pure core + CRUD commands + Hub section + live preview done; master toggle remains).
- ⬜ Per-app writing styles / context.

### Polish & distribution (Phase 4 — ⬜ Planned)
- 🚧 First-run onboarding (hotkey, mic, model download) — `Onboarding.svelte` wizard reusing existing commands; persisted "completed" flag remains.
- 🚧 Settings/"Hub" dashboard with usage stats — first Hub surface + `settings.rs`/`stats.rs` persistence done; some `update_settings` side effects + updater remain.
- ✅ Signed in-app auto-update (`tauri-plugin-updater` + an in-Hub "Atualizar" affordance that surfaces only when a newer signed release exists; minisign-verified).
- ✅ Optional **NVIDIA CUDA** engine (~7–10× faster), downloaded on demand — validated on an RTX 4050.
- 🚧 GitHub release pipeline — `.github/workflows/ci.yml` + `release.yml` exist; signing secrets + updater endpoint remain.

### Backlog (💡)
- Streaming live partials, GPU keep-warm sub-second latency, "Hey MIA" wake word, Whisper Mode (quiet speech), macOS/Linux support, file-transcription mode.

See the full plan in [docs/ROADMAP.md](docs/ROADMAP.md).

---

## How it works

1. Hold your **push-to-talk** hotkey.
2. **Speak.**
3. Release — MIA transcribes, cleans up the text, and types **polished text at your cursor** in the focused app.

Pipeline:

```
hotkey ─▶ cpal (mic, 16 kHz mono) ─▶ Silero VAD (endpoint) ─▶ whisper.cpp (warm model) ─▶ deterministic cleanup ─▶ SendInput (type at cursor)
```

All of this runs in the Rust core — the engine. The Svelte UI is a thin webview used only for settings, onboarding, and the HUD.

---

## Requirements

- **Windows 10 / 11, x64.** (Windows-only for v1 — a deliberate choice; see ADR-011.)
- A microphone.
- **Disk space** for the on-demand Whisper model (downloaded on first use).
- *(Optional)* an **NVIDIA GPU** for the CUDA engine (~7–10× faster transcription), downloaded on demand.

---

## Install

> No release is cut yet — the first installer ships in **Phase 4**. The CI + signed-release **pipeline is already built** (`.github/workflows/ci.yml` and `.github/workflows/release.yml`: `cargo test` + clippy + svelte-check on push/PR, and a `tauri-action` signed Windows installer on `v*` tags), but no version has been tagged/published.

When released, download the **signed installer** from [GitHub Releases](../../releases) and run it. Updates will be delivered via signed in-app auto-update (minisign-verified).

---

## Development

The Tauri 2 + Svelte 5 app lives under [`app/`](app/) (`src/` = the thin Svelte UI, `src-tauri/` = the Rust engine). Run the dev workflow from `app/`.

**Prerequisites**
- [Rust](https://rustup.rs/) (stable, MSVC toolchain)
- [Bun](https://bun.sh/) (dev + build tooling; the shipped app uses WebView2, not Bun). pnpm is the documented fallback if Bun hits a Windows/native-module edge case.
- The **WebView2 runtime** (preinstalled on current Windows 10/11).

**Setup**

```bash
bun install
node scripts/fetch-binaries.mjs   # Windows-only: fetches whisper-server.exe + sibling ggml/whisper DLLs into app/src-tauri/binaries/ (bundled via bundle.resources)
bun run tauri dev
```

---

## Tech stack

Tauri 2 (Rust core) + WebView2 · Svelte 5 (runes) + Vite + TypeScript + Tailwind CSS v4 · whisper.cpp STT (warm whisper-server sidecar — MVP default; whisper-rs in-process later) · cpal audio + Silero VAD · enigo `SendInput` + arboard clipboard injection · `tauri-plugin-global-shortcut` (PTT) + the Tauri `tray-icon` feature. Tooling: Bun.

See [docs/specs/architecture.md](docs/specs/architecture.md) for the architecture decision records.

---

## Privacy

Your voice **never leaves the machine.** Capture, voice activity detection, transcription, cleanup, and text injection all happen locally. There is no cloud service, no account, and no server. The only network access is on-demand downloads of the open Whisper models (and the optional CUDA engine) and the signed update check.

---

## Roadmap

The phased plan lives in [docs/ROADMAP.md](docs/ROADMAP.md).

---

## Docs

- [CLAUDE.md](CLAUDE.md) — project conventions and quick reference.
- [docs/specs/](docs/specs/) — the spec set, including:
  - [architecture.md](docs/specs/architecture.md) — decision records (ADR-001…011).
  - [design-system.md](docs/specs/design-system.md) — the "Calm Focus" design system.
  - [dictation.md](docs/specs/dictation.md) — core orchestration.
  - [speech-to-text.md](docs/specs/speech-to-text.md) — Whisper engine, models, GPU, VAD.
  - [audio-capture.md](docs/specs/audio-capture.md) · [text-injection.md](docs/specs/text-injection.md) · [text-cleanup.md](docs/specs/text-cleanup.md) · [hotkeys.md](docs/specs/hotkeys.md) · [tray-and-hud.md](docs/specs/tray-and-hud.md) · [onboarding.md](docs/specs/onboarding.md) · [settings.md](docs/specs/settings.md) · [custom-dictionary.md](docs/specs/custom-dictionary.md) · [snippets.md](docs/specs/snippets.md) · [ai-commands.md](docs/specs/ai-commands.md)

---

## License

[MIT](LICENSE). MIT app, permissive dependencies only — no AGPL bundled.

### Acknowledgements

- [whisper.cpp](https://github.com/ggerganov/whisper.cpp) (MIT) — the STT engine.
- [OpenAI Whisper](https://github.com/openai/whisper) models (MIT).
- [Silero VAD](https://github.com/snakers4/silero-vad) (MIT) — voice activity detection.
- Design and ergonomics inspired by [Wispr Flow](https://wisprflow.ai).
- Built on patterns from the sibling project **Toolzy** (the owner's Tauri 2 + Rust privacy-first desktop app).
