# <Feature> Feature Spec

> **Status**: Draft | Planned | In progress | Implemented (shipped)
> **Last updated**: YYYY-MM-DD
> **Coverage**: which sections below are filled in (e.g. "Sections 1-4 drafted; 5-9 stubbed")
> **Environment**: desktop (Windows, native)

One-paragraph description: what the feature does, where it sits in the dictation pipeline
(hotkey → capture → VAD → STT → cleanup → inject), and who it's for. Name the Phase it lands in
(see [../ROADMAP.md](../ROADMAP.md)) and the ADR(s) it implements (see
[architecture.md](architecture.md)).

**Scope decisions** (locked at design time — list the deliberate boundaries, each with the
reason and the ADR/Phase it traces to):

- **<decision>**: <what, and why this and not the alternative> (ADR-0XX / Phase X).
- **<decision>**: <what and why>.

---

## 1. Inputs / Outputs

State exactly what goes in and what comes out. MIA is a live dictation app, so frame this around
audio in, text out, and the trigger/target — not file formats.

| Aspect | This feature |
|---|---|
| **Trigger** | <e.g. global push-to-talk hotkey hold; tray action; voice command; UI toggle> |
| **Audio in** | <e.g. cpal 16 kHz mono PCM f32 stream; or N/A if no mic> |
| **Text in** | <e.g. raw transcript from STT; cleaned text from the cleanup module; N/A> |
| **Text out** | <e.g. polished UTF-8 string injected at cursor; settings persisted; N/A> |
| **Target** | <e.g. the OS-focused window via SendInput; the HUD overlay; the Hub window> |
| **Language** | <pt-BR / English / auto-detect / language-agnostic> |

Note which engine/crate backs each path (cpal / Silero VAD / whisper-rs / whisper.cpp /
enigo / arboard / llama.cpp), any latency or size caps, and whether the audio buffer ever
touches disk (default: **no** — voice stays in memory, per ADR-001).

---

## 2. Engine Contract (Rust)

How this feature is implemented in `app/src-tauri` — Rust is the **engine**; the Svelte UI is a
thin webview that only calls typed `invoke()` wrappers (see [architecture.md](architecture.md)).
All commands return `Result<T, String>` — no panics across the IPC boundary (ADR-006).

**Module**: `app/src-tauri/src/<module>.rs`

```rust
#[tauri::command]
async fn foo(state: State<'_, AppState>, opts: Option<FooOpts>) -> Result<FooResult, String>;
// Use a tauri::ipc::Channel<Progress> for streamed events (level meter, partials, download %).
// Long-lived resources (warm whisper model, audio stream, cancel flag) live in managed State.
```

- `FooOpts` / `FooResult` shape (serde `rename_all = "camelCase"`); each field: type, default, effect.
- Which `Err(String)` messages it returns, and when (these map 1:1 to UI error states).
- Native in-process crate (whisper-rs / cpal / enigo) vs runtime-loaded library (CUDA) vs
  optional sidecar fallback (whisper-server) — and any binary/DLL that must be present.
- **Pure helpers** (arg builders, registries, text-cleanup rules, language parsers) live behind
  `#[cfg(test)]` cargo tests and take no I/O — list them here.
- The typed UI wrapper in `app/src/lib/<group>.ts` (`invoke<FooResult>("foo", …)`), one per
  command group; the UI holds **no** dictation logic.

---

## 3. Business Rules

Numbered, testable, unambiguous. Each becomes one or more `cargo test` cases or a manual check.

1. **<rule>** — <precise behavior, including the failure/edge case and the expected result>.
2. ...

---

## 4. Options & Defaults

Every user-facing parameter: name, type, range, default, and effect. Note validation (what the UI
disables/guards vs. what the engine re-checks defensively). Anti-hallucination STT defaults are
**fixed**, not user-tunable (ADR-007) — call that out where relevant.

| Option | Type | Range / values | Default | Effect |
|---|---|---|---|---|
| `<option>` | <bool/enum/int> | <...> | `<default>` | <what it changes> |

---

## 5. Threading / Performance

Live dictation is **latency-critical** — be explicit about threads and the warm-model contract.

- **Audio thread**: cpal callback runs on its own real-time thread; never block it (no STT, no
  injection, no logging-to-disk inside the callback). Hand samples off via a channel/ring buffer.
- **Warm model**: STT model is loaded **once** and kept resident (ADR-004) — whisper-rs in-process
  by default, whisper-server fallback. State that this feature does **not** cold-spawn `whisper-cli`.
- **Latency budget**: <end-to-end target, e.g. utterance-end → first injected char; name the
  dominant cost (STT inference) and what's off the hot path>.
- **Cancellation**: how a release/abort/timeout stops capture and discards in-flight work
  (cancel flag in managed `State`); what partial output (if any) survives.
- **Resource use**: model RAM (CPU build vs CUDA; LLM Q4_K_M ≈1.5-2 GB), and what is lazy-loaded
  and when (model download gate, LLM only on intent match).

---

## 6. UI States

The state machine and which surface owns it — the **floating mic HUD** (dark, translucent,
always-on-top; idle→listening→transcribing→inserting→error) and/or the **Settings/Hub window**
(light theme). See [tray-and-hud.md](tray-and-hud.md) and [design-system.md](design-system.md).

```
States: Idle(hidden) → Listening(pulsing waveform) → Transcribing(spinner)
        → Inserting(brief check) → Idle | Error(message)
Transitions: <hotkey down → Listening; endpoint/release → Transcribing; injected → Inserting; …>
```

- **HUD** (while dictating): per-state visual (waveform level meter, spinner, check, error), the
  single action-blue accent, click-through where possible.
- **Settings/Hub** (if this feature has settings/stats): layout, controls, empty/loading/error.
- Keep the one-action-color discipline and ≥40px hit targets; don't rely on color alone.

---

## 7. Edge Cases

| Scenario | Expected behavior |
|---|---|
| No microphone / mic permission denied | `Err("no input device …")`, HUD error, point to onboarding |
| Model not yet downloaded | gate the action, prompt download (see [speech-to-text.md](speech-to-text.md)) |
| Silence / VAD detects no speech | no injection, return to Idle (no empty/hallucinated text) |
| Focused window is elevated (UAC) | injection may fail silently unless MIA is elevated (ADR-005) — surface it |
| Hotkey released mid-transcription | finish in-flight utterance or cancel per rule; never inject stale text |
| Clipboard fallback used | save & restore the user's prior clipboard contents |
| Another capture already in progress | reject re-entry / queue per rule |

---

## 8. Testing Checklist

- **Rust** (`cargo test`, no I/O — pure helpers only):
  - [ ] pure helpers (arg builders / registries / cleanup rules / parsers) at their boundaries
  - [ ] each `Err(String)` path the command can return
  - [ ] state-machine transitions where they're pure
- **Manual / runtime** (needs mic, model, and a real focused app):
  - [ ] happy path: hotkey → speak → text appears at cursor (pt-BR and English)
  - [ ] HUD reflects every state (listening / transcribing / inserting / error)
  - [ ] cancellation / release mid-utterance behaves per the rules
  - [ ] error messages surface in the HUD/Hub, not just the log
  - [ ] injection into multiple target apps (browser, editor, chat); clipboard restored after fallback

---

## 9. Out of Scope (this version)

- <deferred item> — <why deferred, and which Phase/doc it lands in later (e.g. streaming partials,
  GPU keep-warm, wake word, macOS/Linux — see [../ROADMAP.md](../ROADMAP.md) Phase 5 / Backlog)>.
