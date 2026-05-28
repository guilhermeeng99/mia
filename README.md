# MIA

**Your voice, your machine. Local dictation for Windows.**

Open-source, local, privacy-first voice dictation for Windows — an offline alternative to [Wispr Flow](https://wisprflow.ai).

> **Status**: Phase 0 — documentation & design. **No code exists yet.** This repository currently holds the specs and design system; the app is not built.
> **Last updated**: 2026-05-28
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

Honest status legend: ✅ Done · 🚧 In progress · ⬜ Planned · 💡 Backlog. Today everything is **Planned** — this is a Phase 0 documentation repo.

### Core dictation (Phase 1 — ⬜ Planned)
- Global **push-to-talk** hotkey that works even when MIA is unfocused.
- Live microphone capture (16 kHz mono) with **Silero VAD** endpointing.
- **Warm/resident Whisper** model (loaded once, not respawned per utterance) for low latency.
- **Anti-hallucination defaults, always on**: Silero VAD + greedy decoding (temperature 0, `--no-fallback`) + no previous-text conditioning (`--max-context 0`).
- **Deterministic text cleanup**: filler-word removal (um/uh/é/tipo/né…), spoken-punctuation substitution ("nova linha", "ponto", "vírgula"), stutter/repeat collapse, whitespace normalization, sentence casing.
- **System-wide text injection** at the cursor via Windows `SendInput` (Unicode keystrokes), with a clipboard + Ctrl+V fallback for long text.
- System **tray** icon and a floating **mic HUD** overlay (listening → transcribing → inserting).
- **On-demand model download** gate (small CPU build bundled; models fetched from Hugging Face).
- **pt-BR + English** out of the box.

### AI magic (Phase 2 — ⬜ Planned)
- Optional small **local LLM** (llama.cpp; Qwen2.5-3B / Llama-3.2-3B at Q4_K_M).
- **Command Mode** (voice editing) and an opt-in **Polish** action, gated behind a cheap intent check so average latency stays close to Phase 1.

### Personalization (Phase 3 — ⬜ Planned)
- Custom dictionary / personal vocabulary and word replacement.
- Voice-triggered snippets (text expansion).
- Per-app writing styles / context.

### Polish & distribution (Phase 4 — ⬜ Planned)
- First-run onboarding (hotkey, mic, model download), settings/"Hub" dashboard with stats.
- Signed in-app auto-update.
- Optional **NVIDIA CUDA** engine (~7–10× faster), downloaded on demand.

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

> Not available yet. Installers ship in **Phase 4**.

When released, download the **signed installer** from [GitHub Releases](../../releases) and run it. Updates will be delivered via signed in-app auto-update (minisign-verified).

---

## Development

> The codebase does not exist yet (Phase 0). These are the intended dev prerequisites and workflow.

**Prerequisites**
- [Rust](https://rustup.rs/) (stable, MSVC toolchain)
- [Bun](https://bun.sh/) (dev + build tooling; the shipped app uses WebView2, not Bun). pnpm is the documented fallback if Bun hits a Windows/native-module edge case.
- The **WebView2 runtime** (preinstalled on current Windows 10/11).

**Setup**

```bash
bun install
node scripts/fetch-binaries.mjs   # fetches whisper-cli.exe + sibling ggml/whisper DLLs
bun run tauri dev
```

---

## Tech stack

Tauri 2 (Rust core) + WebView2 · Svelte 5 (runes) + Vite + TypeScript + Tailwind CSS v4 · whisper.cpp STT (warm whisper-rs in-process, whisper-server fallback) · cpal audio + Silero VAD · enigo `SendInput` + arboard clipboard injection · global-hotkey + tray-icon · optional llama.cpp local LLM (Phase 2). Tooling: Bun.

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
