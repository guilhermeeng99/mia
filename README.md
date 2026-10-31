# MIA

**Local voice dictation for Windows and macOS. Your voice, your machine.**

[Website and download](https://guilhermeeng99.github.io/mia/) · [Releases](../../releases)

MIA is a free, open-source desktop app for voice dictation. Hold a hotkey, speak, and MIA types polished text at your cursor in the app you are already using.

Everything runs locally. No cloud account, no speech server, and no voice data leaving your machine.

## Features

- Global push-to-talk hotkey.
- Local transcription with Whisper.
- Multiple dictation languages.
- Interface localization with system-language default.
- Text cleanup for filler words, spoken punctuation, casing, and spacing.
- System-wide text insertion at the cursor.
- Floating microphone HUD and tray / menu bar app.
- Custom dictionary, snippets, and per-app rules.
- GPU acceleration: optional NVIDIA CUDA engine on Windows, Metal built in on Apple Silicon.
- Signed in-app updates.

## Install

Download the latest release from [GitHub Releases](../../releases).

### Windows

Download `MIA_<version>_x64-setup.exe` and run it.

- Windows 10 or 11, x64.
- MIA is not code-signed yet, so SmartScreen may warn "Unrecognized app": click **More info → Run anyway**.
- Optional: an NVIDIA GPU for the CUDA engine (downloaded from the Models view).

Default hotkey: **Ctrl + Space**.

### macOS (experimental)

Download `MIA_<version>_aarch64.dmg`, open it, and drag **MIA** to **Applications**.

- macOS 11 or later on Apple Silicon (M1 or newer). Intel Macs are not supported.
- MIA is not notarized yet, so macOS blocks the first launch. Go to **System Settings → Privacy & Security** and click **Open Anyway**, or run:

  ```bash
  xattr -dr com.apple.quarantine /Applications/MIA.app
  ```

- Grant the two permissions MIA asks for:
  - **Microphone**, to hear you.
  - **Accessibility** (System Settings → Privacy & Security → Accessibility), to paste text into other apps. Without it, dictation transcribes but cannot type.
- Releases are signed with a stable (self-signed) certificate, so the permissions survive updates. Coming from an older, ad-hoc-signed version, macOS asks once more. If typing stops working after an update, remove MIA from the Accessibility list with **−**, then add it again.

Default hotkey: **Option + Space** (Ctrl + Space switches input sources on macOS).

### Everywhere

- A microphone.
- Disk space for the Whisper model, downloaded on first use.

## Development

The desktop app lives in [`app/`](app/).

```bash
cd app
bun install
node scripts/fetch-binaries.mjs
bun run tauri dev
```

`fetch-binaries.mjs` downloads the prebuilt whisper.cpp engine on Windows. On macOS it compiles a static, Metal-enabled `whisper-server` from the pinned whisper.cpp tag, which needs the Xcode Command Line Tools and CMake (`brew install cmake`).

Useful checks:

```bash
cd app
bun run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

Build an installer for the current platform (`.exe` on Windows, `.app` + `.dmg` on macOS):

```bash
cd app
bun run tauri build
```

The `app/` frontend is the Tauri webview for the desktop app. It is not intended to be used as a standalone web app.

### Releases

Every push to `main` runs CI on Windows and macOS, bumps the patch version, and publishes a GitHub Release with the Windows installer, the macOS `.dmg`, and a signed `latest.json` for the in-app updater ([`release.yml`](.github/workflows/release.yml)). The macOS build is appended after the Windows one, so a macOS failure never blocks a Windows release.

macOS builds are signed with the identity in these repository secrets (ad-hoc when they are absent). The workflow imports the certificate into its own keychain, so both a self-signed certificate and an Apple Developer ID work:

| Secret | Purpose |
|---|---|
| `APPLE_CERTIFICATE` | Base64 of the `.p12` signing certificate. |
| `APPLE_CERTIFICATE_PASSWORD` | Password of the `.p12`. |
| `APPLE_SIGNING_IDENTITY` | Certificate name, e.g. `Developer ID Application: Name (TEAMID)`. |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | Notarization. `APPLE_PASSWORD` is an app-specific password. Requires a Developer ID certificate. |

The repository currently uses a self-signed certificate (`MIA Self-Signed Code Signing`, valid until 2036). Any stable certificate keeps the macOS Microphone and Accessibility permissions across updates, because macOS then identifies MIA by bundle ID + certificate instead of by the hash of each build. Keep a backup of the `.p12` and its password: a new certificate is a new identity, and users would grant the permissions once more. A Developer ID certificate with notarization also removes the Gatekeeper warning on first launch.

## Tech

- Tauri 2 + Rust
- Svelte 5 + Vite + TypeScript
- whisper.cpp (CPU / CUDA on Windows, Metal on macOS)
- cpal audio capture
- Silero VAD
- Text insertion: clipboard paste (Ctrl+V / Cmd+V) with SendInput / CGEvent typing as an alternative

## Privacy

MIA processes audio locally. The only network access is for downloading models, downloading the optional CUDA engine, checking signed updates, and opening project links.

## Docs

More detailed specs live in [`docs/specs/`](docs/specs/). Project conventions are in [`CLAUDE.md`](CLAUDE.md).

## License

[MIT](LICENSE)
