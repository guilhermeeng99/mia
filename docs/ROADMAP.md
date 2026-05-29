# MIA — Roadmap

> **Status**: Phase 1 (Core Dictation MVP) in progress — engine modules landing (deterministic cleanup, warm STT, text injection); Phase 0 (Docs & Design) complete.
> **Last updated**: 2026-05-29
> **Environment**: desktop (Windows, native)
> Single source of truth for what is **done**, **in progress**, and **planned**. Update in the same change that shifts scope (see [`/CLAUDE.md`](../CLAUDE.md) → Post-Change Checklist).

MIA is a free, open-source, **privacy-first, fully local** voice-to-text **dictation** app for **Windows** — press a global push-to-talk hotkey, speak, and MIA types polished text at the cursor in whatever app is focused. Everything runs on the user's machine: no cloud, no account, no server, the voice never leaves the device. It is the offline answer to [Wispr Flow](https://wisprflow.ai) (cloud-based). The engine is **Rust** ([Tauri 2](specs/architecture.md#adr-002-tauri-2--svelte-5--vite--tailwind-v4-bun-tooling)); the UI is a thin [Svelte 5](specs/design-system.md) webview for settings/onboarding/HUD only. Brazilian Portuguese (pt-BR) and English are first-class; Whisper covers ~99 languages. See the full decision log in [docs/specs/architecture.md](specs/architecture.md).

## Legend

✅ Done · 🚧 In progress · ⬜ Planned · 💡 Backlog

---

## Status at a glance

| Phase | Theme | Status |
|---|---|---|
| 0 | Docs & Design — this documentation set + design system | ✅ Done |
| 1 | Core Dictation MVP — PTT → capture → VAD → warm STT → cleanup → inject; tray + HUD; pt-BR + English | 🚧 In progress |
| 2 | AI Magic — optional local LLM Command Mode (voice editing) + opt-in Polish; intent routing | ⬜ Planned |
| 3 | Personalization — custom dictionary, snippets, per-app writing styles/context | ⬜ Planned |
| 4 | Polish & Distribution — onboarding, settings/"Hub" dashboard + stats, signed auto-update, NVIDIA CUDA, release pipeline | ⬜ Planned |
| 5 | Backlog — streaming partials, wake word, Whisper Mode, macOS/Linux, file-transcription mode | 💡 Backlog |

---

## Detail

### Phase 0 — Docs & Design ✅

Define the product, architecture, and design system before any code is written. This phase is **complete**; Phase 1 implementation is now underway.

- ✅ Documentation set: [`/README.md`](../README.md), [`/CLAUDE.md`](../CLAUDE.md), this roadmap, [docs/FEATURE-MAP.md](FEATURE-MAP.md) (Wispr Flow → MIA parity + phase), [docs/REUSE-FROM-TOOLZY.md](REUSE-FROM-TOOLZY.md), and all feature specs under [docs/specs/](specs/) following [docs/specs/_template.md](specs/_template.md).
- ✅ [Architecture](specs/architecture.md) — the canonical ADRs (ADR-001 … ADR-011) and the "Rust is the engine, Svelte is a thin webview" principle.
- ✅ [Design system](specs/design-system.md) — the **"Calm Focus"** identity: the light **Settings/Hub** surface (Toolzy tokens + "Sky Blueprint" palette, Montserrat) and the new dark translucent **floating mic HUD**. Shared UI components in `app/src/lib/components/ui/`.

### Phase 1 — Core Dictation MVP 🚧

The end-to-end live loop: global PTT hotkey → cpal capture → Silero VAD endpoint → **warm** whisper.cpp → deterministic cleanup → SendInput injection at the cursor; system tray + floating mic HUD; on-demand model download gate; pt-BR + English. Orchestration spec: [dictation.md](specs/dictation.md). All IPC uses `Result<T, String>` ([ADR-006](specs/architecture.md#adr-006-result-error-model)); pure helpers carry `#[cfg(test)]` cargo tests.

- ✅ **Scaffold** the Tauri 2 + Svelte 5 (runes) + Vite + TypeScript (strict) + Tailwind v4 project with **Bun** tooling (pnpm fallback) and WebView2. Engine modules live under `app/src-tauri/src/*.rs`; the `#[tauri::command]` registry + warm-STT managed state are wired in `lib.rs`; typed `invoke()` wrappers begun in `app/src/lib/*.ts` (`stt.ts`, `inject.ts`). → [architecture.md](specs/architecture.md) · [ADR-002](specs/architecture.md#adr-002-tauri-2--svelte-5--vite--tailwind-v4-bun-tooling)
- 🚧 **Audio capture** — `cpal` mic input at 16 kHz mono PCM, device selection, level metering. Pure DSP core done + cargo-tested (stereo→mono downmix, linear 48→16 kHz resample, `f32`→`s16` quantize, RMS/peak, `FrameChunker`, device-name normalize); `list_input_devices` command live. `app/src-tauri/src/audio.rs`. cpal capture thread (`!Send`-safe) + mono accumulation + `Level` meter + `begin_capture`/`end_capture` (in-process) + `test_microphone` command + Hub mic-test button — compile/build-verified. Remaining (runtime): live Silero per-frame inference + VAD-gated trimming / toggle-endpoint. → [audio-capture.md](specs/audio-capture.md) · [ADR-001](specs/architecture.md#adr-001-native-on-device-privacy-first)
- 🚧 **Silero VAD endpointing** — voice-activity gating + utterance endpoint detection feeding the STT. Endpoint state machine done + cargo-tested (debounce `MIN_SPEECH_MS`, hangover `MIN_SILENCE_MS`, all-silence → nothing, re-arm; Rules 4/5/8), with the fixed Silero constants. `app/src-tauri/src/vad.rs`. Remaining (runtime): Silero model load + per-frame inference producing the probabilities. → [audio-capture.md](specs/audio-capture.md), [speech-to-text.md](specs/speech-to-text.md) · [ADR-007](specs/architecture.md#adr-007-on-demand-models--anti-hallucination)
- ✅ **Warm whisper-server STT** — resident/warm model loaded once via a **whisper-server** sidecar (ADR-004 revised MVP default, cmake-free; **whisper-rs in-process** is the later optimization behind the same seam), NOT a cold `whisper-cli` spawn per utterance. On-demand model download from Hugging Face to app-data with a download gate, `.part` rename, streamed progress via a Tauri `Channel` — **reused/adapted from Toolzy's `transcription.rs`** (MODELS registry, `model_url`/`model_filename`, parsers). Per-request anti-hallucination fixed: greedy (temperature 0, no fallback ladder) + independent per-utterance `/inference`; Silero VAD model fetched alongside the model. Warm lifecycle (spawn/wait/drop), in-memory `transcribe_chunk`, and the `warm_status` command are complete + tested. `app/src-tauri/src/stt.rs`. The engine binary now ships with the installer: `app/scripts/fetch-binaries.mjs` fetches `whisper-server.exe` + sibling ggml/whisper DLLs into `app/src-tauri/binaries/`, and `tauri.conf.json` bundles them via `bundle.resources` (the engine is spawned via `std::process::Command` on a resolved resource path, not a Tauri sidecar). `transcribe_chunk` is now wired into the live dictation orchestrator (`dictation.rs` — capture → warm STT → cleanup → inject, with cancellation); see the *Wire the end-to-end pipeline* item below. Remaining (runtime): live Silero VAD-gated trimming on the capture path. → [speech-to-text.md](specs/speech-to-text.md), [REUSE-FROM-TOOLZY.md](REUSE-FROM-TOOLZY.md) · [ADR-003](specs/architecture.md#adr-003-whisper-stt), [ADR-004](specs/architecture.md#adr-004-warm-resident-stt), [ADR-007](specs/architecture.md#adr-007-on-demand-models--anti-hallucination)
- ✅ **Deterministic cleanup** — a pure Rust module: filler-word stoplist (um/uh/é/tipo/né…), spoken-punctuation substitution ("nova linha", "ponto", "vírgula"), stutter/repeat collapse, whitespace normalization, sentence-case/capitalization fixer, light number spacing, optional trailing period. Always-on, fidelity-safe; per-language (pt-BR/En/Other) rule sets; cargo-tested rules. `app/src-tauri/src/cleanup.rs`. → [text-cleanup.md](specs/text-cleanup.md) · [ADR-008](specs/architecture.md#adr-008-hybrid-text-intelligence)
- 🚧 **Windows text injection** — `enigo` SendInput Unicode keystrokes (default) + `arboard` clipboard + simulated Ctrl+V fallback for long text (save & restore the user's clipboard), behind the `TextInjector` trait with a runtime-selected backend; grapheme-safe chunking + `pick_backend` Auto decision, all cargo-tested; `inject_text` command registered. `app/src-tauri/src/inject.rs`. Remaining: focused-target + elevated-window (UIPI) detection (Rules 6–7, need Win32) wired during the orchestrator stage. → [text-injection.md](specs/text-injection.md) · [ADR-005](specs/architecture.md#adr-005-system-wide-text-injection)
- 🚧 **Global PTT hotkey** — `tauri-plugin-global-shortcut`, push-to-talk that works while unfocused; press-and-hold + toggle modes. Pure core done + cargo-tested: accelerator parser/canonicalizer (round-trips, exact error messages), bare-key + reserved-chord guards, and the debounce + activation-mode `reduce()` reducer (Rules 3/4/9/10). `app/src-tauri/src/hotkey.rs`. Runtime via `tauri-plugin-global-shortcut`: registration + handler (runs `reduce`, emits `dictation://intent`) + startup registration + `register/unregister/update/get_hotkey` commands + `key_to_code`/`to_shortcut` (cargo-tested) + `ptt.ts` frontend wiring — compile/build-verified, validated on Windows. Remaining: `Esc`-cancel transient binding, missing-release watchdog, Settings recorder + conflict-probe. → [hotkeys.md](specs/hotkeys.md) · [ADR-005](specs/architecture.md#adr-005-system-wide-text-injection)
- 🚧 **System tray + floating mic HUD** — `tray-icon` crate for the tray; a dark translucent always-on-top mic HUD with the listening → transcribing → inserting → error state machine and a live waveform/level meter. System tray (Tauri 2 built-in `tray-icon`, `tray.rs`: Open / Quit) wired in setup — compile/build-verified, validated on Windows. HUD `MicHud.svelte` (state machine + RMS waveform) built and shown as a floating overlay in the main window, driven by orchestrator phase events. Remaining (runtime): a dedicated transparent always-on-top HUD *window* (`hud.rs` + `tauri.conf`) + close-to-tray + live waveform `Level` forwarding. → [tray-and-hud.md](specs/tray-and-hud.md), [design-system.md](specs/design-system.md)
- 🚧 **Wire the end-to-end pipeline** — orchestrate hotkey → capture → VAD → warm STT → cleanup → inject, with HUD state transitions and cancel. Pure orchestrator core in `dictation.rs` cargo-tested: `next_phase` HUD state machine (illegal-signal no-op, cancel-from-any), `interpret_down`, `classify_cancel`, `build_result`. The `start/stop/cancel_dictation` commands wire the real pipeline end-to-end (cpal capture → warm STT → cleanup → dictionary → snippets → inject, emitting HUD events + recording stats) + `dictation.ts` — compile/build-verified. The global Ctrl+Space PTT now drives it end-to-end (hotkey → `dictation://intent` → `ptt.ts` → start/stop) and a floating `MicHud` overlay reflects each phase. Remaining (runtime): live HUD waveform `Level` forwarding + toggle auto-endpoint, validated on Windows. → [dictation.md](specs/dictation.md) · [ADR-004](specs/architecture.md#adr-004-warm-resident-stt)
- ✅ **pt-BR + English** — first-class language selection: a Hub picker (Automático / Português (pt-BR) / English) persisted in `settings.general.default_language`; the orchestrator reads it per utterance and forwards `language=` to `/inference` (auto-detect when unset), so the choice is remembered with no warm-engine restart. → [speech-to-text.md](specs/speech-to-text.md) · [ADR-003](specs/architecture.md#adr-003-whisper-stt)

### Phase 2 — AI Magic ⬜

Optional, opt-in **local** intelligence via **llama.cpp** — Qwen2.5-3B-Instruct or Llama-3.2-3B-Instruct at Q4_K_M (~1.5–2 GB RAM), with GBNF/JSON-schema constrained decoding. Gated behind a cheap intent check so average latency stays near Phase 1. `app/src-tauri/src/llm.rs`. → [ai-commands.md](specs/ai-commands.md) · [ADR-008](specs/architecture.md#adr-008-hybrid-text-intelligence)

- ⬜ Local LLM runtime + on-demand model download (same download-gate UX as STT). Pure scaffolding (model-independent prompt/grammar/router) already lives in `ai_commands.rs`; the `llama-cpp-2`/`llama-server` runtime + GGUF download are pending.
- 🚧 **Command Mode** — voice editing ("delete last sentence", "make it formal") via constrained decoding for reliable command parsing. Pure core in `ai_commands.rs` cargo-tested: `command_grammar` (GBNF), `build_prompt`, `validate_parsed`, `ParsedCommand`. Remaining: the constrained-decode runtime + `run_command`.
- ⬜ Opt-in **Polish** action — rewrite/clean beyond the deterministic path, on demand. (`route_intent` already detects the polish phrase; the `polish` command is runtime-pending.)
- 🚧 **Intent routing** — cheap classifier to decide deterministic-only vs. LLM, keeping the fast path default. `route_intent` implemented + cargo-tested (conservative default, pt-BR + en trigger tables) in `ai_commands.rs`.

### Phase 3 — Personalization ⬜

- 🚧 **Custom dictionary** — personal vocabulary / word replacement (names, jargon, acronyms). Pure mechanism-(a) core in `dictionary.rs` cargo-tested (`apply_dictionary` exact/case/whole-word/multi-word/fuzzy/longest-match/idempotent, `match_case`, `fuzzy_match`, `build_bias_prompt`, `validate_entry`; Rules 1-13). CRUD commands + atomic `dictionary.json` persistence + managed state + duplicate rejection + `dictionary.ts` wrapper done (build-verified). Hub dictionary section (add/remove/enable) wired + build-verified. Bias-prompt wiring into the warm-Whisper call is done (fed as the `/inference` initial prompt). → [custom-dictionary.md](specs/custom-dictionary.md)
- 🚧 **Snippets** — voice-triggered text expansion. Pure core in `snippets.rs` cargo-tested (`expand_snippets` whole-phrase/word-boundary/longest-first/no-recursion, `compile_snippets`, `normalize_trigger` case+accent fold, `apply_case`, `validate_snippet`; Rules 1-11). CRUD commands + atomic `snippets.json` persistence + managed state + duplicate-trigger rejection + `preview_expansion` + `snippets.ts` wrapper done (build-verified). Hub snippets section (add/remove + live preview) wired + build-verified. Remaining: the master enable toggle. → [snippets.md](specs/snippets.md)
- ⬜ **Per-app writing styles / context** — style or context selection keyed to the focused application.

### Phase 4 — Polish & Distribution ⬜

- 🚧 **Onboarding** flow — first-run: hotkey, mic, model download. `Onboarding.svelte` wizard (welcome → hotkey → mic test → recommended-model download) + `hotkey.ts` wrapper; shown by `App.svelte` when no model is installed, with skip — build-verified. Remaining: a persisted "completed" flag + permission-denied copy. → [onboarding.md](specs/onboarding.md)
- 🚧 **Settings / "The Hub"** dashboard + usage stats. Shared design-system primitives (`Button`, `Card`, `Field`, `Toggle`, `Pill`) + a first Hub surface (mic device picker, Whisper model list + on-demand download with live progress, warm-engine + GPU status, test-injection) wired to the typed `invoke()` wrappers; build-verified (svelte-check + vite). Backed by a real `settings.rs` persistence layer (single `settings.json`: pure `apply_patch`/`validate`/`migrate`/`parse` core cargo-tested, failure-safe load, atomic save, `get/update/reset_settings` commands + managed state loaded at startup; `settings.ts` wrapper). Local-only usage stats in `stats.rs` (pure WPM + day-streak arithmetic + word count, cargo-tested; `get_stats`/`reset_stats` commands + managed state). Remaining: `update_settings` side effects (hotkey re-register / warm-model swap / launch-at-login), per-dictation stat recording (with the orchestrator), mic-test stream, updater. → [settings.md](specs/settings.md), [design-system.md](specs/design-system.md)
- ⬜ **Signed in-app auto-update** — GitHub Releases + `tauri-plugin-updater`, minisign-verified `latest.json`. → [ADR-009](specs/architecture.md#adr-009-distribution--auto-update)
- ⬜ **NVIDIA CUDA engine** — optional on-demand GPU engine (`nvcuda.dll` detect + download + extract; **≈7–10×** faster), reused/adapted from Toolzy. → [speech-to-text.md](specs/speech-to-text.md), [REUSE-FROM-TOOLZY.md](REUSE-FROM-TOOLZY.md) · [ADR-007](specs/architecture.md#adr-007-on-demand-models--anti-hallucination)
- 🚧 **GitHub release pipeline** — CI (Bun/pnpm build + `cargo test`) → tag → signed Windows installer via `tauri-action` → GitHub Releases. `.github/workflows/ci.yml` (cargo test + clippy `-D warnings` + svelte-check/build on push/PR) and `release.yml` (tauri-action on `v*` tags → draft Release) added. Remaining: set the `TAURI_SIGNING_PRIVATE_KEY` secrets + the updater `latest.json` endpoint. → [ADR-009](specs/architecture.md#adr-009-distribution--auto-update)

---

## Next / open

- ⬜ Finish the Phase 0 documentation set and design system, then start the Phase 1 scaffold.
- ⬜ Decide the warm-STT default in practice: confirm **whisper-rs in-process** meets the latency target, otherwise stay on the **whisper-server** MVP default ([ADR-004](specs/architecture.md#adr-004-warm-resident-stt)).
- ⬜ Validate the SendInput-vs-clipboard injection split against real apps; note the higher-integrity/UAC-window limitation ([ADR-005](specs/architecture.md#adr-005-system-wide-text-injection)).
- ⬜ **Wire live Silero VAD inference** — the `vad.rs` `EndpointDetector` state machine is pure + cargo-tested, but the Silero model load + per-frame inference producing the probabilities that drive it is not wired yet. → [audio-capture.md](specs/audio-capture.md), [speech-to-text.md](specs/speech-to-text.md)
- ✅ **Wire the dictionary bias prompt into warm Whisper** — `build_bias_prompt` output is now fed as the per-utterance `/inference` initial prompt (`stt.rs` `transcribe_chunk`/`inference_fields`, built from the dict snapshot in `dictation.rs`); cargo-tested. → [custom-dictionary.md](specs/custom-dictionary.md), [speech-to-text.md](specs/speech-to-text.md)
- ⬜ **Deferred major dependency bumps** — `cpal` 0.15 → 0.17 and `enigo` 0.5 → 0.6 are held back; both touch Windows device/input behavior and require on-device re-testing before upgrading.
- ⬜ App icons + branding assets (placeholder until then).

## Backlog / ideas

- 💡 **Streaming live partials** — show interim transcription as you speak.
- 💡 **GPU keep-warm** sub-second end-to-end latency.
- 💡 **"Hey MIA" wake word** — hands-free activation.
- 💡 **Whisper Mode** — recognition tuned for quiet/whispered speech.
- 💡 **macOS / Linux** — deferred (macOS Accessibility/TCC; Linux Wayland injection). See [ADR-011](specs/architecture.md#adr-011-windows-only-v1).
- 💡 **File-transcription mode** — batch transcribe audio/video files, reusing Toolzy's ffmpeg + `whisper-cli` file path. → [REUSE-FROM-TOOLZY.md](REUSE-FROM-TOOLZY.md)

## Out of scope (deliberate)

- Any cloud, hosted, or server-side processing — MIA is **fully local**; voice never leaves the machine ([ADR-001](specs/architecture.md#adr-001-native-on-device-privacy-first)).
- Accounts, login, cloud sync, or telemetry.
- Paid tiers / feature gating — MIT, free, open-source ([ADR-010](specs/architecture.md#adr-010-licensing)).
- Mobile (iOS/Android). v1 is **Windows x64 only** ([ADR-011](specs/architecture.md#adr-011-windows-only-v1)).
