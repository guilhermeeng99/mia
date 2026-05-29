# MIA — Project Conventions

> **Status**: Phases 0, 1, and 3 **code-complete**; core loop validated on Windows (PTT → capture → server-side Silero-VAD-gated warm STT → cleanup → inject, pt-BR + English, with the floating HUD). Now also: close-to-tray, toggle-mode auto-endpoint (energy-gated), focused-target + elevated-window (UIPI) injection detection, per-app writing styles, mic-test live meter, and a mic permission-denied deep-link. Phase 4 is code-complete: the release pipeline now **mirrors Toolzy** — `release.yml` auto-bumps/tags/publishes a signed, live installer + `latest.json` on every push to `main` (both signing secrets set), so the **first signed release auto-cuts when this branch merges** (→ `v0.1.1`). App icons/branding now ship — the supplied logo was rasterized into the full icon set. Phase 2 (AI Command Mode / Polish) is **descoped** by product decision — MIA stays a faithful dictation tool. See [`docs/ROADMAP.md`](docs/ROADMAP.md).
> **Last updated**: 2026-05-29
> **Environment**: desktop (Windows, native)

Free, open-source, **privacy-first, local** voice-to-text **dictation** app for **Windows**.
Press a global push-to-talk hotkey, speak, and MIA types polished text at the cursor in
whatever app is focused — everything runs on the user's machine (no cloud, no account, no
server; voice never leaves the device). It is the fully-offline answer to Wispr Flow. Built
with Tauri 2: **Rust is the engine** (audio, VAD, STT, cleanup, injection all run natively);
the **Svelte 5 + TypeScript (Vite)** UI is a thin webview for settings/onboarding/HUD only.
pt-BR and English are first-class; Whisper covers ~99 languages. See
[`README.md`](README.md) for the overview and
[`docs/specs/architecture.md`](docs/specs/architecture.md) for the decision records (ADRs).

Tagline: *"Your voice, your machine. Local dictation for Windows."*

---

## Core Principles

1. **Privacy is a feature, not a footnote.** Everything runs on the user's machine. No
   uploads, no account, no MIA server — ever. Voice never leaves the device. The *only*
   outbound network use is the on-demand model download from Hugging Face (ADR-001).
2. **Native over sandboxed.** Audio capture, VAD, STT, cleanup, and text injection run in
   Rust / bundled native engines, not in a browser. Prefer a compiled-in crate; fall back to
   a sidecar/server only when a crate isn't viable.
3. **Rust is the engine; Svelte orchestrates.** All dictation logic lives in `app/src-tauri`.
   Svelte components hold **no** dictation logic — they call a typed `lib/*` wrapper that
   calls a Rust command via `invoke()` (ADR-002).
4. **Latency-first, faithful-not-creative dictation.** Live dictation uses a **warm/resident**
   STT (model loaded once), not a cold per-utterance spawn (ADR-004). The default path
   transcribes faithfully with anti-hallucination guards always on; intelligence (LLM polish /
   commands) is opt-in and gated (ADR-008).

---

## Architecture

```
app/
  src/                 # Svelte 5 (runes) + TS UI (Vite). Presentation only — no dictation logic.
    lib/
      components/
        ui/            #   shared design-system components (Button, Card, Field, Toggle, Pill, …)
      *.ts             #   typed invoke() wrappers — one per command group (stt, audio,
                       #     hotkey, cleanup, settings, dictionary, snippets, ai …)
    routes/ or App     #   settings/Hub window, onboarding flow, floating mic HUD overlay
  src-tauri/           # Rust = the engine
    src/
      lib.rs           #   Tauri builder + command registry (#[tauri::command] registrations)
      audio.rs         #   cpal mic capture (16 kHz mono PCM)
      vad.rs           #   Silero VAD endpointing
      stt.rs           #   warm whisper (whisper-server sidecar = MVP default; whisper-rs in-process later)
      cleanup.rs       #   deterministic rule-based cleanup (fillers, spoken punctuation, …)
      inject.rs        #   Windows text injection (enigo SendInput + arboard clipboard fallback)
      hotkey.rs        #   global push-to-talk (tauri-plugin-global-shortcut)
      tray.rs          #   system tray (Tauri's built-in tray-icon feature)
      hud.rs           #   floating mic HUD overlay window plumbing
    capabilities/      #   Tauri permissions (scoped)
    tauri.conf.json    #   bundle resources (whisper-server binary + DLLs), window config
  scripts/fetch-binaries.mjs  # auto-fetch whisper binaries + sibling DLLs on Windows
docs/specs/            # per-feature contracts + architecture.md (ADRs)
docs/ROADMAP.md        # done / doing / planned
```

Layer rules:

- **Rust (`src-tauri`)** owns the entire dictation pipeline (hotkey → capture → VAD → STT →
  cleanup → inject) plus tray and HUD. Keep pure, testable logic (arg builders, registries,
  text-cleanup rules) in its own module with `#[cfg(test)]` tests.
- **Svelte (`src`)** never dictates. A component calls a `lib/*` wrapper → a Rust command.
- The STT engine is **warm/resident** for live dictation (ADR-004), not a cold `whisper-cli`
  spawn per utterance — the latency-critical divergence from Toolzy's file mode.

---

## The Engine (Rust commands)

A command = a `#[tauri::command]` that does the work natively and returns `Result<T, String>`.

```rust
#[tauri::command]
async fn transcribe_utterance(
    state: State<'_, SttEngine>,
    samples: Vec<f32>,   // 16 kHz mono PCM from the warm capture path
) -> Result<String, String> { /* run warm whisper, return cleaned text */ }
```

Rules:

- **Commands return `Result<T, String>`** — `Ok` with the payload, `Err` with a short,
  user-presentable message. Map every internal error with `.map_err(|e| format!("…: {e}"))`.
  **Never `panic!` across the IPC boundary** (ADR-006).
- **Pure helpers** (no Tauri, no I/O — arg builders, registries, cleanup rules) live in a
  module and are covered by `cargo test`.
- **Streamed progress** (e.g. model download) uses a Tauri `Channel`; cancellation goes
  through a managed `State`.
- Each command has a typed wrapper in `app/src/lib/*.ts` calling `invoke<T>("cmd", args)`.
  Tauri maps JS camelCase args to Rust snake_case params; structs use serde
  `rename_all = "camelCase"`.
- Components import wrappers, **never** `invoke` directly for logic. Shared visuals live in
  `app/src/lib/components/ui/` — don't duplicate them.

---

## Code Style

- Functions: **5–25 lines**. Split if longer. One responsibility per function/module (SRP).
- Files: ideally under **400–600 lines**.
- Prefer small, composable components / helpers.

### Naming

- Specific and intention-revealing. Avoid generic `data`, `manager`, `handler`, `utils`.
- Searchable and unique within the codebase.

### Control Flow

- Early returns over nesting. Max **2 levels** of indentation.

---

## Comments

- Write **WHY**, not WHAT. Preserve decisions; don't strip meaningful comments in refactors.
- Document exported commands: intent, params, and any runtime requirement (e.g. "needs the
  warm STT engine in managed state").

---

## Key Technologies

| Aspect | Detail |
|---|---|
| Shell | Tauri 2 (Rust) + OS WebView2 |
| UI | Svelte 5 (runes) + TypeScript (`strict`) + Vite |
| Styling | Tailwind CSS v4 (tokens in a single `@theme` block) |
| STT engine | `whisper.cpp` — warm `whisper-server` sidecar (MVP default, cmake-free); `whisper-rs` in-process (later optimization) |
| Models | OpenAI Whisper (MIT), fetched on demand from Hugging Face; small CPU build bundled |
| GPU | optional NVIDIA **CUDA** engine downloaded on demand (~7–10× faster) |
| Anti-hallucination | Silero VAD + greedy (temperature 0) + temperature_inc 0 (disables the fallback ladder) + independent per-request `/inference` (no cross-utterance context) — whisper-server, not whisper-cli flags |
| Audio capture | `cpal` (16 kHz mono PCM) |
| VAD / endpointing | Silero VAD |
| Hotkey | `tauri-plugin-global-shortcut` (push-to-talk, works unfocused) |
| Tray | Tauri's built-in `tray-icon` feature |
| Text injection | `enigo` (SendInput Unicode) default + `arboard` + Ctrl+V fallback (Windows) |
| AI (Phase 2) | local LLM via `llama.cpp` (Qwen2.5-3B / Llama-3.2-3B, Q4_K_M) — GBNF-constrained |
| Distribution | GitHub Releases + `tauri-plugin-updater` (minisign-verified `latest.json`) |
| Platform | **Windows x64 only** for v1 (ADR-011); macOS/Linux deferred |
| Package manager | **Bun** (dev + build only; shipped app uses WebView2). pnpm = documented fallback |
| Lint / format | Biome + Prettier; `cargo clippy` for Rust |
| Tests | `cargo test` (Rust pure helpers) · UI is thin |
| License | MIT app; permissive deps only — **never bundle AGPL** (ADR-010) |

---

## Commands

```bash
# app/ (the desktop app)
bun install
bun run tauri dev        # run the desktop app (Vite + Tauri)
bun run build            # svelte-check + vite build
bun run tauri build      # Windows installer (needs icons + fetched binaries)
node scripts/fetch-binaries.mjs   # fetch whisper binaries + sibling DLLs (Windows)
cargo test --manifest-path src-tauri/Cargo.toml   # Rust unit tests
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings   # release CI gate (warnings = errors)
```

> `scripts/fetch-binaries.mjs` runs under Bun or Node.
> pnpm is the documented fallback if Bun hits a Windows/native-module edge case.
> Note: on Windows, the RTK hook can mask `biome`/`bun` output; prefer the PowerShell tool to
> see real lint/build results.

---

## Post-Change Checklist

After every change:

1. `cargo test` (if Rust touched) — green; new pure logic has tests, bug fixes get a regression test.
2. `cargo clippy --manifest-path app/src-tauri/Cargo.toml --all-targets -- -D warnings` (if Rust touched) — zero warnings. **The release CI gates on this**; `cargo test` alone won't catch clippy lints, and a red clippy silently blocks the release.
3. `bun run build` in `app/` — svelte-check + Vite green.
4. Biome / Prettier — zero errors.
5. Touched a feature's behavior? Update its spec in `docs/specs/` in the same change.
6. Shipped/started/finished a roadmap item? Update [`docs/ROADMAP.md`](docs/ROADMAP.md).

---

## Spec-Driven Development

Every feature has a contract at `docs/specs/<feature>.md` **before** new code. Use
[`docs/specs/_template.md`](docs/specs/_template.md). Sections: Overview + scope decisions
(locked), Business rules (numbered, testable), Engine contract (the `#[tauri::command]`
signatures, `Result<T, String>`), Options & defaults, Threading/Performance, UI states (state
machine), Edge cases, Testing checklist, Out of scope. Keep spec and code in sync.

---

## Privacy & Security

- **Voice never leaves the device.** No telemetry that inspects voice or text. No uploads, no
  MIA server in any path (ADR-001).
- The **one** outbound network use is the **on-demand model download** from Hugging Face
  (Whisper models + optional CUDA engine) — gated behind explicit user action (ADR-007).
- Any future context-reading feature **excludes password fields** and sensitive inputs.
- Ship a **restrictive CSP**; the webview is settings/onboarding/HUD only and loads no remote
  content.
- Note: synthetic input (SendInput) can't reach higher-integrity (elevated/UAC) windows unless
  MIA itself is elevated (ADR-005).

---

<!-- rtk-instructions v2 -->
# RTK (Rust Token Killer) - Token-Optimized Commands

## Golden Rule

**Always prefix commands with `rtk`**. If RTK has a dedicated filter, it uses it. If not, it passes through unchanged. This means RTK is always safe to use.

**Important**: Even in command chains with `&&`, use `rtk`:
```bash
# ❌ Wrong
git add . && git commit -m "msg" && git push

# ✅ Correct
rtk git add . && rtk git commit -m "msg" && rtk git push
```

## RTK Commands by Workflow

### Build & Compile (80-90% savings)
```bash
rtk cargo build         # Cargo build output
rtk cargo check         # Cargo check output
rtk cargo clippy        # Clippy warnings grouped by file (80%)
rtk tsc                 # TypeScript errors grouped by file/code (83%)
rtk lint                # ESLint/Biome violations grouped (84%)
rtk prettier --check    # Files needing format only (70%)
rtk next build          # Next.js build with route metrics (87%)
```

### Test (60-99% savings)
```bash
rtk cargo test          # Cargo test failures only (90%)
rtk go test             # Go test failures only (90%)
rtk jest                # Jest failures only (99.5%)
rtk vitest              # Vitest failures only (99.5%)
rtk playwright test     # Playwright failures only (94%)
rtk pytest              # Python test failures only (90%)
rtk rake test           # Ruby test failures only (90%)
rtk rspec               # RSpec test failures only (60%)
rtk test <cmd>          # Generic test wrapper - failures only
```

### Git (59-80% savings)
```bash
rtk git status          # Compact status
rtk git log             # Compact log (works with all git flags)
rtk git diff            # Compact diff (80%)
rtk git show            # Compact show (80%)
rtk git add             # Ultra-compact confirmations (59%)
rtk git commit          # Ultra-compact confirmations (59%)
rtk git push            # Ultra-compact confirmations
rtk git pull            # Ultra-compact confirmations
rtk git branch          # Compact branch list
rtk git fetch           # Compact fetch
rtk git stash           # Compact stash
rtk git worktree        # Compact worktree
```

Note: Git passthrough works for ALL subcommands, even those not explicitly listed.

### GitHub (26-87% savings)
```bash
rtk gh pr view <num>    # Compact PR view (87%)
rtk gh pr checks        # Compact PR checks (79%)
rtk gh run list         # Compact workflow runs (82%)
rtk gh issue list       # Compact issue list (80%)
rtk gh api              # Compact API responses (26%)
```

### JavaScript/TypeScript Tooling (70-90% savings)
```bash
rtk pnpm list           # Compact dependency tree (70%)
rtk pnpm outdated       # Compact outdated packages (80%)
rtk pnpm install        # Compact install output (90%)
rtk npm run <script>    # Compact npm script output
rtk npx <cmd>           # Compact npx command output
rtk prisma              # Prisma without ASCII art (88%)
```

### Files & Search (60-75% savings)
```bash
rtk ls <path>           # Tree format, compact (65%)
rtk read <file>         # Code reading with filtering (60%)
rtk grep <pattern>      # Search grouped by file (75%). Format flags (-c, -l, -L, -o, -Z) run raw.
rtk find <pattern>      # Find grouped by directory (70%)
```

### Analysis & Debug (70-90% savings)
```bash
rtk err <cmd>           # Filter errors only from any command
rtk log <file>          # Deduplicated logs with counts
rtk json <file>         # JSON structure without values
rtk deps                # Dependency overview
rtk env                 # Environment variables compact
rtk summary <cmd>       # Smart summary of command output
rtk diff                # Ultra-compact diffs
```

### Infrastructure (85% savings)
```bash
rtk docker ps           # Compact container list
rtk docker images       # Compact image list
rtk docker logs <c>     # Deduplicated logs
rtk kubectl get         # Compact resource list
rtk kubectl logs        # Deduplicated pod logs
```

### Network (65-70% savings)
```bash
rtk curl <url>          # Compact HTTP responses (70%)
rtk wget <url>          # Compact download output (65%)
```

### Meta Commands
```bash
rtk gain                # View token savings statistics
rtk gain --history      # View command history with savings
rtk discover            # Analyze Claude Code sessions for missed RTK usage
rtk proxy <cmd>         # Run command without filtering (for debugging)
rtk init                # Add RTK instructions to CLAUDE.md
rtk init --global       # Add RTK to ~/.claude/CLAUDE.md
```

## Token Savings Overview

| Category | Commands | Typical Savings |
|----------|----------|-----------------|
| Tests | vitest, playwright, cargo test | 90-99% |
| Build | next, tsc, lint, prettier | 70-87% |
| Git | status, log, diff, add, commit | 59-80% |
| GitHub | gh pr, gh run, gh issue | 26-87% |
| Package Managers | pnpm, npm, npx | 70-90% |
| Files | ls, read, grep, find | 60-75% |
| Infrastructure | docker, kubectl | 85% |
| Network | curl, wget | 65-70% |

Overall average: **60-90% token reduction** on common development operations.
<!-- /rtk-instructions -->
