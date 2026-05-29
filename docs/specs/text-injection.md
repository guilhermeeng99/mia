# Text Injection Feature Spec

> **Status**: Phase 1 — implemented & wired: both backends (`SendInput` + clipboard save/restore), the `pick_backend` / `chunk_for_sendinput` / `should_use_clipboard` / `redact_for_log` pure helpers, and the `inject_text` command (registered in `lib.rs`), all cargo-tested. Runtime-pending: focused-target + elevated-window (UIPI) detection (Rules 6–7), wired in the orchestrator stage.
> **Last updated**: 2026-05-29
> **Coverage**: Sections 1-9 drafted
> **Environment**: desktop (Windows, native)

Text injection is the **last stage** of the dictation pipeline (hotkey → capture → VAD → STT →
cleanup → **inject**): it takes the cleaned UTF-8 transcript and types it into **whatever app is
currently focused** — a browser, an editor, a chat box — with no per-app integration. The text is
arbitrary Unicode (pt-BR accents, em-dashes, emoji) and ranges from a few words to long paragraphs.
This is a Windows-specific, latency-sensitive, and security-sensitive stage: it synthesizes input
into another process's foreground window. It lands in **Phase 1 — Core Dictation MVP** (see
[../ROADMAP.md](../ROADMAP.md)) and implements **ADR-005** (system-wide text injection on Windows;
see [architecture.md](architecture.md#adr-005-system-wide-text-injection-on-windows)). It is invoked
by the dictation orchestrator ([dictation.md](dictation.md)); its behavior is configurable from the
Hub ([settings.md](settings.md)).

**Scope decisions** (locked at design time):

- **Two backends behind one Rust trait, runtime-selected**: `enigo` `SendInput` Unicode keystrokes
  (default) and `arboard` clipboard + simulated `Ctrl+V` (fallback / forced). One `TextInjector`
  trait localizes all OS-specific code and keeps the door open for future per-OS backends
  (ADR-005 / ADR-011).
- **SendInput Unicode is the default** because it works in the widest set of controls, types
  arbitrary Unicode scalars regardless of the user's keyboard layout (`KEYEVENTF_UNICODE`), and
  does **not** touch the user's clipboard (ADR-005).
- **Clipboard fallback MUST save and restore the user's prior clipboard** — leaving the clipboard
  exactly as it was is a hard requirement, not a nicety (ADR-005 / architecture.md).
- **No per-app text rules in v1** — injection is target-agnostic. Per-app writing styles/context
  are Phase 3 (see [../ROADMAP.md](../ROADMAP.md)).
- **No retrieval of existing document content** — MIA writes at the cursor; it never reads the
  focused control's text (privacy + UIPI; ADR-001 / ADR-005).
- **Synthetic input cannot reach higher-integrity (elevated/UAC) windows unless MIA itself runs
  elevated** — this is a Windows UIPI limit, not a bug; MIA surfaces it instead of failing silently
  (ADR-005).

---

## 1. Inputs / Outputs

| Aspect | This feature |
|---|---|
| **Trigger** | Called by the dictation orchestrator after cleanup (utterance-end → cleaned text ready); also a UI "test injection" action in the Hub |
| **Audio in** | N/A — this stage is past STT |
| **Text in** | Cleaned UTF-8 `String` from the text-cleanup module ([text-cleanup.md](text-cleanup.md)); plus a `mode` selector and the active settings |
| **Text out** | Keystrokes / paste delivered to the OS-focused window via `SendInput`; **no** value returned on success (`Ok(())`) |
| **Target** | The OS foreground/focused editable control (best-effort detection); falls back to clipboard-only + notify if no editable target |
| **Language** | Language-agnostic — operates on Unicode scalars, not words |

Backing crates: **`enigo`** (`SendInput` with `KEYEVENTF_UNICODE`) for the default backend;
**`arboard`** (clipboard get/set) + `enigo` (`Ctrl+V` chord) for the fallback. No model, no network,
no audio. Nothing here touches disk — the transcript stays in memory (ADR-001), and password-field
content is **never** read, stored, or logged (see Edge Cases).

---

## 2. Engine Contract (Rust)

Rust is the **engine**; the Svelte UI is a thin webview that calls one typed `invoke()` wrapper and
holds no injection logic (see [architecture.md](architecture.md)). All commands return
`Result<T, String>` — no panics across the IPC boundary (ADR-006). In normal dictation the
orchestrator calls the injector **directly in Rust**; the `#[tauri::command]` exists for the Hub's
"test injection" button and for forcing a mode.

**Module**: `app/src-tauri/src/inject.rs`

```rust
/// Runtime-selected backend. Default = SendInput; Clipboard = forced or auto-fallback.
#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InjectMode { Auto, SendInput, Clipboard }

/// The one trait that localizes OS-specific injection (ADR-005 / ADR-011).
pub trait TextInjector: Send + Sync {
    fn inject(&self, text: &str) -> Result<(), String>;
    fn name(&self) -> &'static str; // "send_input" | "clipboard"
}

pub struct SendInputInjector { /* enigo handle */ }
pub struct ClipboardInjector { /* arboard handle + chunk size */ }

#[tauri::command]
async fn inject_text(
    state: tauri::State<'_, AppState>,
    text: String,
    mode: InjectMode,            // default Auto
) -> Result<(), String>;
```

- **`inject_text(text, mode)`** — entry point. `Auto` picks `SendInput` unless settings force
  clipboard, the text exceeds `clipboard_threshold_chars`, or `SendInput` reports failure;
  `Clipboard`/`SendInput` force the named backend. Empty/whitespace-only `text` → `Ok(())` no-op.
- **`SendInputInjector::inject`** — feeds `text` to `enigo` as `KEYEVENTF_UNICODE` events,
  **chunked** (see pure helper) with a small inter-chunk yield so a long paragraph doesn't monopolize
  the input queue or drop events. Surrogate pairs and combining marks are emitted as separate
  Unicode events in order.
- **`ClipboardInjector::inject`** — (1) **read & save** the user's current clipboard via `arboard`;
  (2) set clipboard to `text`; (3) synthesize `Ctrl+V` via `SendInput`; (4) **restore** the saved
  clipboard. Each step is a distinct fallible op (see Rule 5, Rule 9).
- **`Err(String)` messages** (map 1:1 to UI states): `"no editable target focused"`,
  `"target window is elevated; run MIA as administrator to type into it"`,
  `"clipboard unavailable"`, `"clipboard restore failed"`, `"injection backend failed"`.
- **Pure helpers** (no I/O, behind `#[cfg(test)]` cargo tests):
  - `chunk_for_sendinput(text: &str, max: usize) -> Vec<&str>` — splits on char boundaries (never
    mid-grapheme / mid-surrogate), respecting `max` chars per chunk.
  - `pick_backend(mode, len, settings) -> Backend` — the Auto decision (force-clipboard setting,
    length threshold) as a pure function.
  - `should_use_clipboard(len, threshold) -> bool`.
  - `redact_for_log(text: &str) -> &'static str` — returns a length-only placeholder; injection text
    is **never** logged verbatim.
- **UI wrapper**: `app/src/lib/inject.ts` → `invoke<void>("inject_text", { text, mode })`. Used only
  by the Hub test action; live dictation never round-trips through the webview.

---

## 3. Business Rules

1. **Default backend is SendInput Unicode** — in `Auto`, MIA injects via `enigo` `SendInput` with
   `KEYEVENTF_UNICODE`, so any Unicode scalar (pt-BR `ç`/`ã`, em-dash, emoji) types correctly
   regardless of the active keyboard layout. No clipboard is touched on this path.
2. **Clipboard fallback is used when** (a) the user forces it (`force_clipboard_mode`), OR (b) the
   text length ≥ `clipboard_threshold_chars`, OR (c) a `SendInput` attempt fails. `Auto` resolves
   this via `pick_backend`.
3. **Clipboard save/restore is mandatory** — the clipboard backend reads and saves the user's prior
   clipboard contents before writing, and restores them after `Ctrl+V`. The user's clipboard MUST
   end exactly as it started on the success path (ADR-005).
4. **Restore even on failure** — if the `Ctrl+V` paste fails after the clipboard was overwritten,
   MIA still attempts to restore the saved clipboard before returning the error.
5. **Clipboard restore failure is surfaced, not swallowed** — if restore fails, return
   `Err("clipboard restore failed")` so the HUD/Hub can warn the user their clipboard may hold the
   dictated text (Rule 12 forbids logging the text itself).
6. **No editable target → clipboard-only + notify** — if best-effort detection finds no focused
   editable control, MIA does **not** blindly synthesize keystrokes into the void; it copies the
   text to the clipboard and shows a HUD/tray message ("Copied — no text field focused. Paste with
   Ctrl+V."). It still saves/restores? No — in this branch the copy IS the deliverable, so the
   clipboard is intentionally left holding the text and the user is told. Returns `Ok(())`.
7. **Elevated target → clear message** — if the focused window is higher-integrity (elevated/UAC)
   and MIA is not elevated, `SendInput` silently no-ops at the OS level. MIA detects this (best
   effort, e.g. attempt + foreground-integrity check) and returns
   `Err("target window is elevated; run MIA as administrator to type into it")`, shown in the HUD.
8. **Empty / whitespace-only text is a no-op** — returns `Ok(())`, injects nothing (cleanup may have
   reduced an utterance to nothing; VAD-silence should already prevent this — see
   [text-cleanup.md](text-cleanup.md)).
9. **Long text is chunked** — SendInput injection splits text into `chunk_for_sendinput` pieces on
   char boundaries with a brief yield between chunks, so long paragraphs don't overflow the input
   queue or interleave with the user's own typing.
10. **Char-boundary safety** — chunking never splits a UTF-8 grapheme, surrogate pair, or
    combining-mark sequence; emoji and accented clusters arrive intact.
11. **Forced clipboard mode setting** — when `force_clipboard_mode` is on, MIA always uses the
    clipboard backend (still with save/restore) regardless of length.
12. **Never store or log the injected text** — the transcript is treated as sensitive. Logs record
    only backend name, char count, and outcome via `redact_for_log`. Password-field detection (Rule
    13) tightens this further.
13. **Password fields are never special-cased into storage** — MIA does not read field content. If
    the focused control is a known password input, MIA still types via SendInput (so the user can
    dictate a password if they choose) but **never** routes it through the clipboard backend (which
    would briefly place it on the clipboard) and **never** logs even its length.
14. **One injection at a time** — injection for an utterance must complete (or error) before the next
    utterance's injection begins; the orchestrator serializes calls (see [dictation.md](dictation.md)).

---

## 4. Options & Defaults

| Option | Type | Range / values | Default | Effect |
|---|---|---|---|---|
| `inject_mode` | enum | `auto` / `sendInput` / `clipboard` | `auto` | Backend selection passed to `inject_text` |
| `force_clipboard_mode` | bool | true/false | `false` | When on, always use clipboard backend (Rule 11) |
| `clipboard_threshold_chars` | int | 200–5000 | `1000` | At/over this length, `Auto` uses the clipboard backend (Rule 2b) |
| `sendinput_chunk_chars` | int | 16–512 | `64` | Chars per `SendInput` chunk (Rule 9) |
| `restore_clipboard` | bool | true/false | `true` | Save & restore prior clipboard on the clipboard path. **Should not be disabled**; exposed for diagnostics only |

Validation: the UI clamps `clipboard_threshold_chars` and `sendinput_chunk_chars` to their ranges;
the engine re-clamps defensively (`pick_backend` / `chunk_for_sendinput` treat out-of-range as the
default). `restore_clipboard=false` shows a warning in the Hub. These are injection options only —
STT anti-hallucination defaults are fixed elsewhere (ADR-007; see
[speech-to-text.md](speech-to-text.md)).

---

## 5. Threading / Performance

- **Not on the audio thread** — injection runs after STT + cleanup, well off the cpal real-time
  callback; the audio thread is never blocked by `SendInput`/clipboard I/O.
- **No model, no spawn** — this stage loads nothing and does not cold-spawn `whisper-cli`; it is pure
  Win32 input synthesis (the warm-model contract lives in [speech-to-text.md](speech-to-text.md) /
  ADR-004).
- **Latency budget**: injection should be a small fraction of the end-to-end utterance-end → first
  visible char budget; the dominant cost is STT inference, not typing. SendInput chunking adds only
  tiny inter-chunk yields; the clipboard path adds two clipboard ops + one paste round-trip
  (still well under perceptible delay for typical text).
- **Clipboard race window**: between "set clipboard" and "restore clipboard" there is a brief window
  where the user's clipboard holds the dictated text and another app's clipboard write could collide.
  Minimize it: set → paste → restore back-to-back; keep the window as short as practical; never
  block on user input inside it. Restore failures are surfaced (Rule 5).
- **Cancellation**: if the orchestrator cancels mid-utterance (hotkey released, abort), no injection
  call is made — stale text is never typed (see [dictation.md](dictation.md)). An in-progress chunked
  SendInject is allowed to finish the current call (it's short); the cancel applies to the next
  utterance.
- **Resource use**: negligible — no model RAM, no GPU. The clipboard backend holds at most one saved
  clipboard payload in memory transiently.

---

## 6. UI States

Injection is the **Inserting** state of the dictation HUD state machine (dark, translucent,
always-on-top). See [tray-and-hud.md](tray-and-hud.md) and [design-system.md](design-system.md).

```
… → Transcribing(spinner) → Inserting(brief check) → Idle
                                   └→ Error(message)   (no target / elevated / clipboard failure)
Transitions: cleaned text ready → Inserting; inject Ok(()) → brief check → Idle;
             inject Err → Error(message) shown in HUD, then Idle
```

- **HUD** (while dictating): `Inserting` shows the brief action-blue check; an `Err` flips to the
  error state with a one-line message ("Window is elevated — run MIA as admin", "Copied to clipboard
  — no field focused"). Single action color; ≥40px hit targets on any actionable toast; never rely on
  color alone (icon + text).
- **Settings/Hub** (light theme): exposes the Section 4 options plus a **"Test injection"** button
  (types a sample string into a focus-following target / shows the result), and a clear note about the
  elevated-window limitation (ADR-005).

---

## 7. Edge Cases

| Scenario | Expected behavior |
|---|---|
| No editable target focused | Copy text to clipboard, notify ("Copied — no text field focused"), `Ok(())` (Rule 6) |
| Focused window is elevated (UAC) | `Err("target window is elevated; run MIA as administrator…")`, HUD error (Rule 7, ADR-005) |
| Password field focused | Type via SendInput only; never use clipboard backend; never log length or content (Rule 13) |
| Very long text | Use clipboard backend at/over `clipboard_threshold_chars`; SendInput path chunks (Rules 2b, 9) |
| Clipboard unavailable (locked by another app) | `Err("clipboard unavailable")`; fall back to SendInput if mode was `Auto` and text fits |
| Clipboard restore fails | Attempt restore, then `Err("clipboard restore failed")`; warn user clipboard may hold the text (Rules 4, 5) |
| Empty / whitespace-only text | `Ok(())`, no keystrokes (Rule 8) |
| User starts typing during injection | Chunked SendInput minimizes interleave; no lock on the foreground — best effort, documented |
| Emoji / surrogate pairs / combining accents | Emitted as ordered Unicode events; chunking respects grapheme boundaries (Rule 10) |
| Force-clipboard setting on | Always clipboard backend, with save/restore (Rule 11) |

---

## 8. Testing Checklist

- **Rust** (`cargo test`, no I/O — pure helpers only):
  - [x] `chunk_for_sendinput` — splits at the chunk size; never mid-surrogate / mid-grapheme; emoji
        and `ç`/`ã`/combining marks stay intact; empty input → empty/`[""]` per contract
  - [x] `pick_backend` — `Auto` picks SendInput below threshold, clipboard at/over threshold, and
        clipboard when `force_clipboard_mode`; `SendInput`/`Clipboard` force their backend
  - [x] `should_use_clipboard` at the threshold boundary (n-1, n, n+1)
  - [x] `redact_for_log` returns no verbatim text (length-only)
  - [ ] each `Err(String)` variant is produced by the right branch (where testable without Win32)
- **Manual / runtime** (needs a real focused app):
  - [ ] happy path: dictate → text appears at cursor in Notepad, VS Code, a browser field, a chat box
        (pt-BR accents + emoji render correctly)
  - [ ] long paragraph: clipboard backend used; **user's prior clipboard is restored** afterward
  - [ ] force-clipboard mode: every injection restores the prior clipboard
  - [ ] clipboard restore failure path surfaces a HUD/Hub warning
  - [ ] elevated target (e.g. Task Manager / an admin console): clear "run as admin" message, no silent loss
  - [ ] no focus / desktop: text copied to clipboard + notification, `Ok(())`
  - [ ] password field: types via SendInput, never via clipboard; nothing logged
  - [ ] HUD reflects Inserting → Idle and Error states

---

## 9. Out of Scope (this version)

- **macOS / Linux injection** — different APIs (macOS Accessibility/TCC, Linux Wayland synthetic-input
  restrictions); deferred behind the same `TextInjector` trait (ADR-011; [../ROADMAP.md](../ROADMAP.md)
  Phase 5 / Backlog).
- **Per-app text rules / writing styles** — target-specific formatting and context are Phase 3
  ([../ROADMAP.md](../ROADMAP.md)).
- **Reading the focused control's existing text** — MIA only writes at the cursor; it never retrieves
  document content (privacy + UIPI; ADR-001 / ADR-005).
- **Auto-elevation / UAC prompting** — MIA does not request elevation on its own; the user runs it as
  admin if they want to dictate into elevated windows. Only the message/guidance is in scope.
- **Undo / re-injection of a prior utterance from this module** — orchestration concern; see
  [dictation.md](dictation.md).
