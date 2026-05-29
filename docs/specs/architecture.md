# Architecture & Decision Records

> **Status**: Phase 1 (Core Dictation MVP) in progress — engine modules landing; ADRs accepted
> **Last updated**: 2026-05-29
> **Environment**: desktop (Windows, native)
> Cross-cutting design + the rationale (ADRs) behind the big choices. Feature-level contracts
> live in sibling specs (see the [file list](#cross-references)); start new ones from
> [`_template.md`](_template.md).

MIA is a **free, open-source, privacy-first, local** voice-to-text **dictation** app for
**Windows**. Press a global hotkey (push-to-talk), speak, and MIA types polished text at the
cursor in whatever app is focused. Everything runs on the user's machine — no cloud, no
account, no server, voice never leaves the device. It is an open-source answer to
[Wispr Flow](https://wisprflow.ai), with the key difference that MIA is **fully offline/local**.
This document records the system shape and the decisions (ADRs) that pin it down.

---

## 1. System shape

MIA is a **native desktop application** (Tauri 2). One app: a thin Svelte webview for
settings/onboarding/HUD over a Rust engine that does all of the dictation work. The Rust core
(`app/src-tauri`) is the **engine**; the Svelte UI (`app/src`) is a **thin webview** that holds
no dictation logic and only calls typed `invoke()` wrappers (one per command group in
`app/src/lib/*.ts`).

```
              ┌──────────────────────────────────────────┐
              │        app/src  (Svelte 5 + Vite + TS)    │  ← thin webview UI
              │  Settings/Hub (light) · Onboarding ·       │    (WebView2)
              │  Floating mic HUD (dark)                   │
              │  components → lib/*.ts invoke() wrappers    │
              └─────────────────────┬──────────────────────┘
                                    │ invoke() (IPC)  ·  Result<T, String>
              ┌─────────────────────▼──────────────────────┐
              │       app/src-tauri  (Rust) — THE ENGINE     │
              │   #[tauri::command]s · audio · VAD · STT ·    │
              │   cleanup · injection · hotkey · tray         │
              └───┬───────────────────────────────────┬─────┘
       in-process │                                   │ system / OS-level
   ┌──────────────▼───────────────────┐   ┌───────────▼───────────────────────┐
   │ resident / loaded:                 │   │ OS interaction:                    │
   │  • cpal            (16 kHz mic)    │   │  • enigo / SendInput  (inject text)│
   │  • whisper-rs      (WARM STT)      │   │  • arboard            (clipboard)  │
   │    └ or whisper-server (sidecar)   │   │  • tauri-plugin-global-shortcut    │
   │  • Silero VAD      (endpointing)   │   │      (global PTT hotkey)           │
   │  • llama.cpp       (Phase 2 LLM)   │   │  • Tauri tray-icon feature (tray)  │
   └────────────────────────────────────┘   └────────────────────────────────────┘
```

**The latency-critical seam** is the warm STT (see [ADR-004](#adr-004-warmresident-stt-for-live-dictation)):
the model is loaded **once** and stays resident, so dictation never pays a cold model-load per
utterance. The UI/IPC seam is the set of Rust **commands**; the UI renders controls and calls a
typed wrapper, owning no dictation logic.

**Outbound network.** The default dictation path is **fully offline**. The **only** outbound
network use is (1) the **on-demand model download** from Hugging Face (Whisper / Silero weights,
and the optional NVIDIA CUDA engine), Rust-side and user-initiated; and (2) the **updater's
startup version check** against GitHub Releases ([ADR-009](#adr-009-distribution--signed-in-app-auto-update)).
Voice audio and transcripts never leave the machine.

---

## ADRs

### ADR-001: Native, on-device, privacy-first
**Status:** Accepted · 2026-05-28

**Context.** Dictation means a microphone is always one hotkey away from recording the user's
voice. Cloud dictation tools (e.g. Wispr Flow) stream audio to a server and require an account.
For a tool that sits resident in the system tray and listens on demand, trust is the product:
the user must *know* their voice never leaves the machine.

**Decision.** Do **all** work — audio capture, VAD, STT, cleanup, text injection — **natively
on the user's machine**. No cloud, no account, no server, no telemetry. Voice audio and
transcripts never cross the network. The app is usable airplane-mode after the one-time model
download.

**Consequences.**
- ✅ Strong privacy by construction — there is no server to leak, subpoena, or breach.
- ✅ Zero per-use cost and no rate limits; works offline.
- ✅ Native latency (in-process STT) instead of network round-trips.
- ⚠️ STT quality/speed is bounded by the user's CPU/GPU, not a datacenter; mitigated by the
  optional CUDA engine ([ADR-007](#adr-007-on-demand-model-download--cpu-bundled--optional-cuda-engine)).
- ⚠️ Per-OS native builds + a Rust toolchain; models are large and must be fetched on demand.

### ADR-002: Tauri 2 (Rust engine) + Svelte 5 + Vite + Tailwind v4 UI
**Status:** Accepted · 2026-05-28

**Context.** The app needs a small footprint, a native Rust core for the latency-sensitive
audio/STT pipeline, and a lightweight UI surface for settings, onboarding, and a floating HUD.

**Decision.** Single desktop app: **Tauri 2** (Rust) for the core; **Svelte 5 (runes) +
TypeScript (strict) + Vite + Tailwind CSS v4** for the UI rendered in the OS **WebView2**.
Tooling/package manager is **Bun** (dev + build only — the shipped app uses WebView2, not Bun);
**pnpm** is the documented fallback for Windows/native-module edge cases. Tailwind tokens live
in a single `@theme` block (see [design-system.md](design-system.md)).

**Consequences.**
- ✅ Small installers (OS WebView2, not Electron); the Rust core does the heavy lifting.
- ✅ Svelte 5 runes give a reactive UI with minimal runtime; Vite is the natural Tauri front-end.
- ✅ Bun is fast for dev/build; the shipped binary carries no Bun runtime.
- ⚠️ UI logic is constrained to `invoke()` calls — by design, the dictation engine lives in Rust.
- ⚠️ WebView2 is a Windows runtime dependency (present on modern Windows; the bootstrapper
  installs it if missing).

### ADR-003: Whisper (whisper.cpp) as the STT engine
**Status:** Accepted · 2026-05-28

**Context.** MIA's first-class languages are **pt-BR (Brazilian Portuguese)** and **English**,
with broad coverage beyond. The engine must be local, MIT-clean, and faithful for pt-BR.

**Decision.** Use **OpenAI Whisper via whisper.cpp** as the STT engine. Whisper covers ~99
languages, is MIT-licensed (engine and model weights), and transcribes **pt-BR** faithfully.

**Why not Parakeet / Canary (NVIDIA).** They are faster and lead in English, but per NVIDIA's
own model cards they are trained on **European** Portuguese — weaker pt-BR — and Parakeet is
ASR-only. For faithful pt-BR plus integration simplicity, Whisper wins.

**Relation to Toolzy.** This continues the engine choice of **Toolzy's ADR-010** (Whisper
chosen for the same pt-BR fidelity reason). MIA reuses Toolzy's
`app/src-tauri/src/transcription.rs` — the `MODELS` registry, `model_url`/`model_filename`, the
on-demand Hugging Face download, the `detected_language`/`parse_whisper_progress` parsers, and
the anti-hallucination arg builder. The **divergence**: Toolzy runs a cold `whisper-cli` sidecar
per file; MIA runs a **warm/resident** model for live dictation
([ADR-004](#adr-004-warmresident-stt-for-live-dictation)). See [speech-to-text.md](speech-to-text.md)
and [REUSE-FROM-TOOLZY.md](../REUSE-FROM-TOOLZY.md).

**Consequences.**
- ✅ Faithful pt-BR + English + ~99 languages; MIT-clean engine and weights.
- ✅ Reuses Toolzy's proven model registry, download, and parser code.
- ⚠️ Whisper's failure mode is **hallucination** (inventing/looping text over silence/noise);
  fixed anti-hallucination defaults are mandatory ([ADR-007](#adr-007-on-demand-model-download--cpu-bundled--optional-cuda-engine)).

### ADR-004: Warm/resident STT for live dictation
**Status:** Accepted · 2026-05-28

**Context.** Dictation is **interactive**: the user expects text to appear within a beat of
releasing the hotkey. The dominant cost in a naive Whisper invocation is **loading the model
into memory** (hundreds of ms to several seconds for larger models). Toolzy spawns a fresh
`whisper-cli` process per file and pays that load every time — fine for a one-shot file
transcription, **fatal** for repeated short utterances. This is the single latency-critical
divergence from Toolzy, and the hardest part of the system.

**Decision.** Keep the STT model **resident (warm)**: load it **once** and reuse it for every
utterance. Two backends sit behind one internal `SttBackend` trait, runtime-selected:

- **MVP default — `whisper-server` sidecar.** whisper.cpp's bundled HTTP server runs as a local
  sidecar with the model loaded **once**; MIA POSTs captured PCM/WAV to `127.0.0.1` and reads the
  text back. It needs **no C++/cmake build** — a prebuilt binary is fetched (reusing Toolzy's
  `fetch-binaries.mjs` pattern), so it builds out of the box on the reference machine (which has
  no cmake) and still satisfies the warm requirement: each utterance pays **inference only**, not
  a model reload. The localhost HTTP hop is negligible for short dictation utterances.
- **Optimization — `whisper-rs` in-process.** The whisper.cpp model linked directly into MIA's
  process, so each utterance is pure inference with **zero IPC hop** — the lowest-latency option.
  Deferred because `whisper-rs` builds whisper.cpp via **cmake** (absent on the reference machine;
  the CUDA variant also needs the CUDA toolkit). Enabled later behind the same trait once the
  C++/cmake chain is set up.

> **Revised 2026-05-28** from the original "in-process default" after the build-toolchain audit
> found **cmake absent**. The `SttBackend` trait makes the default a swap, not a rewrite, so the
> in-process optimization can land later without touching the dictation pipeline.

The warm model has an explicit **lifecycle**:
- **load** — on first dictation (or eagerly after onboarding's model-download gate); reports
  progress to the UI via a Tauri `Channel`.
- **resident** — held in a Tauri managed `State` (e.g. behind a `Mutex`/`OnceCell`); subsequent
  utterances skip loading entirely.
- **swap** — changing model or engine (CPU↔CUDA) tears down and reloads; the UI shows a brief
  "loading model" state.
- **unload** — optionally evicted on an idle timer or low-memory signal to free ~0.5–3 GB RAM,
  then lazily reloaded on the next hotkey press.

Anti-hallucination defaults still apply on every utterance (Silero VAD + greedy decoding with the
temperature-fallback ladder disabled + stateless, independent `/inference` calls that carry no
cross-utterance context; see [ADR-007](#adr-007-on-demand-model-download--cpu-bundled--optional-cuda-engine)).

**Consequences.**
- ✅ **Sub-utterance latency** — repeated dictations pay inference only, not model load.
- ✅ `whisper-rs` in-process is the lowest-latency option (no IPC/HTTP hop, no WAV round-trip).
- ✅ The `whisper-server` sidecar is the cmake-free **MVP default** — a prebuilt binary (Toolzy
  fetch pattern), no C++ build; the shared trait makes swapping in `whisper-rs` later additive.
- ⚠️ A resident model holds significant **RAM** for the app's lifetime — hence the
  unload-on-idle option and a sensible default model size.
- ⚠️ In-process inference must run **off the UI/command path** on a dedicated worker so audio
  capture and the HUD stay responsive (see [Threading](#threading-audio-thread-vs-command-execution)).
- ⚠️ `whisper-rs` links whisper.cpp at build time (and per-GPU-variant); the `whisper-server`
  fallback exists precisely for when that link is impractical.

See [speech-to-text.md](speech-to-text.md) and [dictation.md](dictation.md).

### ADR-005: System-wide text injection on Windows
**Status:** Accepted · 2026-05-28

**Context.** MIA must type the recognized text into **whatever app is focused** — a browser,
an editor, a chat box — with no integration per target app. The text is arbitrary Unicode
(pt-BR accents, em-dashes, emoji) and ranges from a few words to long paragraphs. This is the
other novel/hard part of the system, and it is Windows-specific.

**Decision.** Inject text via a Rust **trait** with a **runtime-selected backend**, default
plus fallback:

1. **Default — `enigo` `SendInput` Unicode keystrokes.** Synthesize keystrokes carrying the
   actual Unicode scalar (`KEYEVENTF_UNICODE`), independent of the user's keyboard layout. This
   behaves like real typing, lands at the caret, and works in the vast majority of apps.
2. **Fallback — `arboard` clipboard + simulated `Ctrl+V`** for long text. For large blocks,
   keystroke-by-keystroke injection is slow and can drop characters under load; instead place
   the text on the clipboard and synthesize a paste. Crucially, **save the user's existing
   clipboard before, and restore it after** — the user's clipboard must be left exactly as it
   was. A heuristic (text length / target-app quirks) selects this path.

The chosen backend is decided at runtime; pure arg/decision helpers carry `#[cfg(test)]` cargo
tests.

**Known limitation — UAC / integrity levels.** Synthetic input from a normal-integrity process
**cannot reach higher-integrity (elevated / UAC) windows** (UIPI). If the focused window is an
elevated app, injection silently fails unless MIA itself is elevated. MIA runs unelevated by
default (least privilege); this is surfaced to the user rather than worked around, and running
MIA elevated is an explicit opt-in.

**Consequences.**
- ✅ Works system-wide with **no per-app integration** — any focused text field receives text.
- ✅ `SendInput` Unicode is layout-independent and handles pt-BR accents/emoji correctly.
- ✅ The clipboard fallback makes long paragraphs fast and lossless; save/restore keeps the
  user's clipboard untouched.
- ⚠️ Cannot type into elevated windows unless MIA is elevated (UIPI) — documented, not hidden.
- ⚠️ Clipboard paste depends on the target honoring `Ctrl+V`; the trait lets us add per-app
  rules later.
- ⚠️ Some apps debounce/throttle synthetic input; the length heuristic and the clipboard path
  mitigate dropped characters.

See [text-injection.md](text-injection.md).

### ADR-006: Result<T, String> error model across the Rust ↔ UI IPC
**Status:** Accepted · 2026-05-28

**Context.** A panic across the Tauri IPC boundary is unrecoverable and opaque to the UI. The
engine has many fallible steps (no mic, model not downloaded, injection blocked) that the UI
must present clearly.

**Decision.** Every `#[tauri::command]` returns **`Result<T, String>`**: an `Ok` payload, or a
short user-presentable `Err` message. No panics cross the boundary; `invoke()` rejects on `Err`
and the UI shows the message.

**Consequences.**
- ✅ Predictable, presentable failure surface; trivial to display in the HUD/Hub.
- ✅ Engine code uses `?`/`map_err` to funnel errors to a string at the command edge.
- ⚠️ Less structured than a typed error enum — acceptable for a single-app UI; revisit if the
  UI needs to branch on error *kind* (e.g. "mic missing" vs "model missing").

### ADR-007: On-demand model download + CPU bundled + optional CUDA engine
**Status:** Accepted · 2026-05-28

**Context.** Whisper model weights are far too large to bundle in the installer, and not every
user has an NVIDIA GPU. Whisper's hallucination failure mode (inventing/looping text over
silence or noise) must be suppressed by default.

**Decision.** Bundle a small **CPU** whisper build for an out-of-the-box experience; download
larger models **on demand** from **Hugging Face** to the app-data dir (with a `.part` →
final-name rename on completion). Offer an **optional NVIDIA CUDA engine** downloaded on demand
(**~7–10× faster**), detected via `nvcuda.dll`. CPU stays the bundled default/fallback. The
**anti-hallucination defaults are fixed and always on**: **Silero VAD** (only detected speech
reaches Whisper) + **greedy decoding with the temperature-fallback ladder disabled** (each
`/inference` request to `whisper-server` uses `temperature = 0` and `temperature_inc = 0`, which
turns off whisper's temperature-fallback ladder — the server-side equivalent of `whisper-cli`'s
`--no-fallback`) + **no previous-text conditioning** (each `/inference` call is independent and
stateless, so no transcript from a prior utterance is fed forward — the server-side equivalent of
`--max-context 0`). The literal `--no-fallback` / `--max-context 0` flags are `whisper-cli` flags
and are neither used nor needed with the `whisper-server` sidecar.

**Relation to Toolzy.** This **lifts Toolzy's ADR-010** transcription machinery
(`transcription.rs`): the `MODELS` registry, `model_url`/`model_filename`, the `ureq` HF
download with `.part` rename, the Silero VAD constants, the anti-hallucination `whisper_args`
builder, the progress `Channel`, cancel-via-managed-`State`, and the CUDA detect
(`nvcuda.dll`)/download/extract flow. The **adaptation**: MIA feeds the warm model from **cpal
mic capture** ([audio-capture.md](audio-capture.md)) instead of a file preprocessed by ffmpeg.
See [REUSE-FROM-TOOLZY.md](../REUSE-FROM-TOOLZY.md).

**Consequences.**
- ✅ Small installer; users pick the fidelity/speed they need; faithful, hallucination-resistant
  transcripts by default.
- ✅ Reuses Toolzy's registry/URL/arg builders (pure → `cargo test`) and the CUDA engine path.
- ✅ Optional CUDA engine gives a large speedup for NVIDIA users without affecting the CPU
  default.
- ⚠️ Outbound network for the one-time model/engine download (weights only — never audio),
  Rust-side via `ureq`, user-initiated behind an onboarding **download gate**
  ([onboarding.md](onboarding.md)).
- ⚠️ The `whisper-server` sidecar ships as `whisper-server.exe` + its sibling `ggml`/`whisper`
  DLLs on Windows. `app/scripts/fetch-binaries.mjs` (Toolzy's `fetchExeWithDlls` pattern) fetches
  the prebuilt `whisper-server.exe` and DLLs into `app/src-tauri/binaries/`, and
  `tauri.conf.json` → `bundle.resources` ships them inside the installer. (The future `whisper-rs`
  in-process backend would instead link whisper.cpp at build time — see ADR-004.)

See [speech-to-text.md](speech-to-text.md).

### ADR-008: Hybrid text intelligence — deterministic cleanup (Phase 1) + optional local LLM (Phase 2)
**Status:** Accepted · 2026-05-28

**Context.** Raw Whisper output contains filler words (um/uh/é/tipo/né), spoken punctuation
("nova linha", "ponto", "vírgula"), stutters/repeats, and inconsistent casing. Users also want
voice *editing* ("delete that sentence", "make it formal") — but an LLM in the hot path adds
latency and can rewrite faithfully-dictated text in unwanted ways.

**Decision.** **Two-tier, fidelity-safe by default.**
- **Phase 1 (always on) — deterministic, rule-based cleanup**: a pure Rust module — filler-word
  stoplist, spoken-punctuation substitution, stutter/repeat collapse, whitespace normalization,
  and a sentence-case/capitalization fixer. No model, no latency, fully testable
  ([text-cleanup.md](text-cleanup.md)).
- **Phase 2 (optional) — small local LLM via llama.cpp**: Qwen2.5-3B-Instruct or
  Llama-3.2-3B-Instruct at **Q4_K_M** (~1.5–2 GB RAM) for **Command Mode** (voice editing) and
  an opt-in **"Polish"** action, using **GBNF / JSON-schema constrained decoding** for reliable
  command parsing. Gated behind a **cheap intent check** so average latency stays near Phase 1
  ([ai-commands.md](ai-commands.md)).

**Consequences.**
- ✅ The default path is **faithful, not creative**, and pays no LLM latency.
- ✅ Intelligence is opt-in and available locally — still no cloud.
- ✅ Constrained decoding makes command parsing reliable instead of free-text-fragile.
- ⚠️ A second large local model (the LLM) adds RAM/download cost — gated and optional.
- ⚠️ The intent router must be cheap and accurate enough that Phase 2 doesn't regress Phase 1
  latency for ordinary dictation.

### ADR-009: Distribution & signed in-app auto-update
**Status:** Accepted · 2026-05-28

**Context.** A desktop app distributed outside an app store still needs a safe way to ship fixes
without users re-downloading installers.

**Decision.** Ship via **GitHub Releases** and bundle **`tauri-plugin-updater`**. On launch the
app checks a `latest.json` on GitHub; release artifacts are **minisign-signed** in CI and
verified against the embedded public key before installing. A failed/offline check is swallowed
(never throws to the UI).

**Consequences.**
- ✅ One-click signed updates; the signature check rejects tampered artifacts.
- ⚠️ The signing key must stay secret — losing it breaks the update chain for installed apps.
- ⚠️ One outbound request to GitHub at startup (a version check — no audio, no file contents);
  this is one of the only two outbound uses in the app.

### ADR-010: Licensing — MIT app, permissive deps only
**Status:** Accepted · 2026-05-28

**Context.** MIA is open-source and must stay legally clean to distribute freely.

**Decision.** The app is **MIT**. Use **permissive dependencies only** and **never bundle
AGPL**. The core stack is permissive: whisper.cpp (MIT), Whisper model weights (MIT), Silero VAD
(MIT), and cpal / enigo / arboard (permissive), plus `tauri-plugin-global-shortcut` (the global
PTT hotkey) and Tauri's built-in **tray-icon feature** (the system tray) — both permissive,
shipped under the Tauri umbrella. Native engines run
in-process (statically/dynamically linked permissive code) or as separate-process sidecars.

**Consequences.**
- ✅ MIA can be distributed freely with no copyleft contamination.
- ✅ Mirrors Toolzy's licensing discipline (its ADR-005) — permissive-only, never AGPL.
- ⚠️ Some otherwise-attractive components (AGPL) are off-limits and must be replaced with
  permissive equivalents.

### ADR-011: Windows-only v1 (deliberate)
**Status:** Accepted · 2026-05-28

**Context.** System-wide text injection and global hotkeys are deeply OS-specific. Windows
offers the simplest injection story (`SendInput`) and is the owner's platform. macOS requires
Accessibility/TCC permission prompts and a different injection API; Linux Wayland actively
restricts synthetic input.

**Decision.** Target **Windows x64 only for v1**, deliberately. Defer macOS (Accessibility/TCC)
and Linux (Wayland injection) to the backlog. The injection trait ([ADR-005](#adr-005-system-wide-text-injection-on-windows))
keeps the door open for future per-OS backends without restructuring the engine.

**Consequences.**
- ✅ One platform → the simplest injection path (`SendInput`), one capture stack, one CI target;
  ships sooner and works reliably.
- ✅ The injection/hotkey traits localize the OS-specific code, so adding a backend later is
  additive.
- ⚠️ No macOS/Linux in v1 — explicitly out of scope, tracked in [ROADMAP.md](../ROADMAP.md)
  Phase 5 / Backlog.

---

## Cross-cutting concerns

### Warm-model lifecycle
The STT model is the app's most expensive resource and the reason dictation feels instant
([ADR-004](#adr-004-warmresident-stt-for-live-dictation)). It moves through
**load → resident → swap → unload**:
- **Load** happens once — eagerly after the onboarding download gate, or lazily on first
  dictation — with progress streamed to the UI over a Tauri `Channel`.
- **Resident** state lives in a Tauri managed `State` (behind a `Mutex`/`OnceCell`); every
  subsequent utterance is inference-only.
- **Swap** (model change, or CPU↔CUDA engine change) tears down and reloads behind a brief UI
  "loading model" state.
- **Unload** is an optional idle/low-memory eviction that frees RAM and reloads lazily on the
  next press.
The `whisper-rs` in-process backend and the `whisper-server` sidecar fallback sit behind one
internal trait, runtime-selected.

### Threading (audio thread vs command execution)
Three concurrency domains must not block each other:
- **Audio capture** runs on **cpal's real-time audio callback thread** — it must never block
  (no model load, no allocation storms); it hands 16 kHz mono PCM frames to a channel.
- **STT inference** runs on a **dedicated worker** (not the audio thread, not the UI/command
  thread) so a long transcription never stalls capture or the HUD.
- **Tauri commands** execute off the UI thread; long work (model load, transcription) reports
  progress via `Channel` and is **cancellable** via a managed `State` cancel flag (reusing
  Toolzy's cancel pattern). The global hotkey and tray callbacks dispatch onto this machinery
  rather than doing work inline.

### The injection trait + clipboard save/restore
Text injection ([ADR-005](#adr-005-system-wide-text-injection-on-windows)) is a Rust **trait**
with a runtime-selected backend: `enigo`/`SendInput` Unicode by default, `arboard` clipboard +
simulated `Ctrl+V` for long text. The clipboard path **must save the user's current clipboard
before pasting and restore it immediately after** — leaving the user's clipboard exactly as it
was is a hard requirement, not a nicety. Decision/arg helpers are pure and carry `#[cfg(test)]`
cargo tests. The trait also localizes the UIPI/elevation limitation and leaves room for future
per-app rules and per-OS backends.

### Privacy
No telemetry, no uploads, no MIA server in any path; **voice audio and transcripts never leave
the device**. The default dictation path is fully offline. Exactly **two** outbound request
types exist, both direct from the user's machine and unrelated to voice content: (1) the
**updater's startup version check** against GitHub Releases ([ADR-009](#adr-009-distribution--signed-in-app-auto-update)),
and (2) the **on-demand model download** from Hugging Face — Whisper/Silero weights and the
optional CUDA engine — Rust-side via `ureq`, user-initiated behind the onboarding download gate
([ADR-007](#adr-007-on-demand-model-download--cpu-bundled--optional-cuda-engine)). The webview
runs under a restrictive CSP (`tauri.conf.json` → `app.security.csp`), with no remote scripts.

### Future: file-transcription mode (reusing Toolzy's ffmpeg path)
A backlog **file-transcription mode** ([ROADMAP.md](../ROADMAP.md) Phase 5) would let users drop
an audio/video file and get a transcript — the original Toolzy use case. It would **reuse
Toolzy's ffmpeg preprocessing path** (decode/resample any input to 16 kHz mono WAV) feeding the
same warm Whisper model, rather than the live cpal capture path. The STT engine, model registry,
and anti-hallucination flags are shared; only the *front end* of the pipeline (file + ffmpeg vs
live mic + VAD) differs. This is explicitly out of scope for v1.

---

## Cross-references

- [README.md](../../README.md) · [CLAUDE.md](../../CLAUDE.md)
- [ROADMAP.md](../ROADMAP.md) · [FEATURE-MAP.md](../FEATURE-MAP.md) · [REUSE-FROM-TOOLZY.md](../REUSE-FROM-TOOLZY.md)
- Specs: [dictation.md](dictation.md) · [speech-to-text.md](speech-to-text.md) ·
  [audio-capture.md](audio-capture.md) · [text-injection.md](text-injection.md) ·
  [text-cleanup.md](text-cleanup.md) · [hotkeys.md](hotkeys.md) · [tray-and-hud.md](tray-and-hud.md) ·
  [onboarding.md](onboarding.md) · [settings.md](settings.md) ·
  [custom-dictionary.md](custom-dictionary.md) · [snippets.md](snippets.md) ·
  [ai-commands.md](ai-commands.md) · [design-system.md](design-system.md) · [_template.md](_template.md)
