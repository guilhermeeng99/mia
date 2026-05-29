# MIA

**Your voice, your machine. Local dictation for Windows.**

Open-source, local, privacy-first voice dictation for Windows — an offline alternative to [Wispr Flow](https://wisprflow.ai).

> **Status**: Phase 1 (Core Dictation MVP) — **in progress.** The Tauri 2 + Svelte 5 app is scaffolded and the engine modules are landing: warm whisper-server STT, deterministic cleanup, text injection, global PTT hotkey, system tray, and the end-to-end dictation loop are wired; Phase 0 (Docs & Design) is complete. See [docs/ROADMAP.md](docs/ROADMAP.md) for per-feature status.
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
- 🚧 Global **push-to-talk** hotkey that works even when MIA is unfocused (default Ctrl+Space). Runtime registration + the press/hold + toggle reducer are wired; recorder/conflict-probe remain.
- 🚧 Live microphone capture (16 kHz mono) — pure DSP core + `list_input_devices`/`test_microphone` done; live Silero per-frame inference remains.
- 🚧 **Silero VAD** endpointing — endpoint state machine done + tested; the live model load/inference remains.
- ✅ **Warm/resident Whisper** model (loaded once via a warm **whisper-server** sidecar, not respawned per utterance) for low latency.
- **Anti-hallucination, always on**: Silero VAD + greedy decoding (whisper-server `/inference` at temperature 0 with the temperature-fallback ladder disabled) + stateless, independent per-utterance requests (no previous-text conditioning across utterances).
- ✅ **Deterministic text cleanup**: filler-word removal (um/uh/é/tipo/né…), spoken-punctuation substitution ("nova linha", "ponto", "vírgula"), stutter/repeat collapse, whitespace normalization, sentence casing.
- 🚧 **System-wide text injection** at the cursor via Windows `SendInput` (Unicode keystrokes), with a clipboard + Ctrl+V fallback for long text (`inject_text` live; focused-target / elevated-window detection remains).
- 🚧 System **tray** icon (Open / Quit, wired) and a floating **mic HUD** overlay (listening → transcribing → inserting; `MicHud.svelte` built, dedicated HUD window remains).
- ✅ **On-demand model download** gate with streamed progress (models fetched from Hugging Face).
- ⬜ **pt-BR + English** first-class language selection (the pt-BR-faithful Whisper path).

### AI magic (Phase 2 — ⬜ Planned)
- Optional small **local LLM** (llama.cpp; Qwen2.5-3B / Llama-3.2-3B at Q4_K_M). Pure scaffolding (prompt/grammar/intent router) is in `ai_commands.rs`; the runtime is pending.
- **Command Mode** (voice editing) and an opt-in **Polish** action, gated behind a cheap intent check so average latency stays close to Phase 1.

### Personalization (Phase 3 — ⬜ Planned)
- 🚧 Custom dictionary / personal vocabulary and word replacement (pure core + CRUD commands + Hub section done; bias-prompt wiring remains).
- 🚧 Voice-triggered snippets (text expansion) (pure core + CRUD commands + Hub section + live preview done; master toggle remains).
- ⬜ Per-app writing styles / context.

### Polish & distribution (Phase 4 — ⬜ Planned)
- 🚧 First-run onboarding (hotkey, mic, model download) — `Onboarding.svelte` wizard reusing existing commands; persisted "completed" flag remains.
- 🚧 Settings/"Hub" dashboard with usage stats — first Hub surface + `settings.rs`/`stats.rs` persistence done; some `update_settings` side effects + updater remain.
- ⬜ Signed in-app auto-update.
- ⬜ Optional **NVIDIA CUDA** engine (~7–10× faster), downloaded on demand.
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
node scripts/fetch-binaries.mjs   # fetches whisper-server.exe + sibling ggml/whisper DLLs
bun run tauri dev
```

---

## Tech stack

Tauri 2 (Rust core) + WebView2 · Svelte 5 (runes) + Vite + TypeScript + Tailwind CSS v4 · whisper.cpp STT (warm whisper-server sidecar — MVP default; whisper-rs in-process later) · cpal audio + Silero VAD · enigo `SendInput` + arboard clipboard injection · `tauri-plugin-global-shortcut` (PTT) + the Tauri `tray-icon` feature · optional llama.cpp local LLM (Phase 2). Tooling: Bun.

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
