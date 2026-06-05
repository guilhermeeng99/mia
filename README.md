# MIA

**Local voice dictation for Windows. Your voice, your machine.**

[Website and download](https://guilhermeeng99.github.io/mia/) · [Releases](../../releases)

MIA is a free, open-source Windows desktop app for voice dictation. Hold a hotkey, speak, and MIA types polished text at your cursor in the app you are already using.

Everything runs locally. No cloud account, no speech server, and no voice data leaving your machine.

## Features

- Global push-to-talk hotkey.
- Local transcription with Whisper.
- Multiple dictation languages.
- Interface localization with system-language default.
- Text cleanup for filler words, spoken punctuation, casing, and spacing.
- System-wide text insertion at the cursor.
- Floating microphone HUD and tray app.
- Custom dictionary, snippets, and per-app rules.
- Optional NVIDIA CUDA engine for faster transcription.
- Signed in-app updates.

## Install

Download the latest Windows installer from [GitHub Releases](../../releases).

Requirements:

- Windows 10 or 11, x64.
- A microphone.
- Disk space for the Whisper model downloaded on first use.
- Optional: NVIDIA GPU for CUDA acceleration.

## Development

The desktop app lives in [`app/`](app/).

```bash
cd app
bun install
node scripts/fetch-binaries.mjs
bun run tauri dev
```

Useful checks:

```bash
cd app
bun run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

The `app/` frontend is the Tauri webview for the desktop app. It is not intended to be used as a standalone web app.

## Tech

- Tauri 2 + Rust
- Svelte 5 + Vite + TypeScript
- whisper.cpp
- cpal audio capture
- Silero VAD
- Windows SendInput and clipboard fallback

## Privacy

MIA processes audio locally. The only network access is for downloading models, downloading the optional CUDA engine, checking signed updates, and opening project links.

## Docs

More detailed specs live in [`docs/specs/`](docs/specs/). Project conventions are in [`CLAUDE.md`](CLAUDE.md).

## License

[MIT](LICENSE)
