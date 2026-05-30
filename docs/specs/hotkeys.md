# Hotkeys Feature Spec

> **Status**: Phase 1 — pure core + runtime both implemented. Pure (cargo-tested): `parse_accelerator`/`to_canonical`, `is_bare_key`/`is_reserved`, `reduce` (Rules 3/4/9/10), and `key_to_code`/`to_shortcut`. Runtime (compile/build-verified, validated on Windows): registration via `tauri-plugin-global-shortcut`, the handler that runs `reduce` and emits `dictation://intent`, startup registration (Rule 14), and the `register/unregister/update/get_hotkey` commands. The frontend (`ptt.ts`) drives the orchestrator off the intent. The `Esc`-cancel transient binding (Rule 13 — `Escape` registered only while a session is active, routed by the plugin handler) and the missing-release watchdog (Rule 11 — push-to-hold force-Stop after `MAX_HOLD_MS`, disarmed by a session generation) are now implemented in the runtime. The Hub hotkey-recorder (capture a modifier+key chord) + activation-mode picker are wired to `update_hotkey`, whose registration step is the conflict-probe (rejects an already-claimed chord, keeps the prior binding). **Self-healing registration (Rule 15)** is now implemented — `start_self_heal` (idle re-register tick, cargo-tested `should_self_heal` predicate), `request_reregister` (tray "Reativar atalho" + Hub-focus), and `power_resume.rs` (resume/unlock watcher) re-claim a silently-dropped `RegisterHotKey` routing so `Ctrl+Space` no longer goes dead until the app is restarted. Pending: broad on-device chord testing only.
> **Last updated**: 2026-05-29
> **Coverage**: Sections 1-9 drafted.
> **Environment**: desktop (Windows, native)

The hotkey layer is the **trigger** at the very front of the dictation pipeline
(**hotkey** → capture → VAD → STT → cleanup → inject). It registers a single global,
push-to-talk (PTT) shortcut that fires even when MIA is **not** the focused window, and
translates raw key down/up events into a small set of clean, debounced *dictation intents*
(`Start` / `Stop`) that drive the orchestration state machine in
[`dictation.md`](dictation.md). It owns no audio, STT, or injection logic — it is purely the
"finger on the trigger". It is backed by the `tauri-plugin-global-shortcut` plugin (permissive
license, ADR-010) and lands in **Phase 1 — Core Dictation MVP** (see [../ROADMAP.md](../ROADMAP.md)).
It implements
[ADR-001](architecture.md) (native, on-device), [ADR-006](architecture.md) (`Result<T,String>`
IPC), and the Windows-only scope of [ADR-011](architecture.md).

**Scope decisions** (locked at design time):

- **One global PTT chord, not a key map.** v1 registers exactly **one** dictation hotkey. We are
  not building a configurable multi-action keymap (toggle vs. command-mode vs. polish on separate
  keys) in Phase 1; Command Mode/Polish (Phase 2) reuse the *same* trigger and route by intent.
  Reason: a single, reliable trigger is the MVP; more keys = more conflicts and UX surface (Phase 1).
- **Two activation modes only.** `push-to-hold` (dictate while held, finish on release) and
  `press-to-toggle` (press to start, press again to stop). No "tap to toggle, hold to dictate"
  hybrid in v1 — it conflicts with debounce edge handling and is hard to discover (Phase 1).
- **Default chord = `Ctrl + Space`, rebindable (locked default).** Reason: low collision risk,
  ergonomic for hold, and **registrable by `tauri-plugin-global-shortcut`** — a modifier-only chord
  (e.g. `Ctrl+Win`) is *not* registrable via `RegisterHotKey`, so the default carries a real key.
  (`DEFAULT_ACCEL = "Ctrl+Space"` in `hotkey.rs`; aligned with [settings.md](settings.md).)
  **`Fn` is not a default option** — it is a firmware/EC-level key that does not reliably surface as
  a Windows virtual-key and cannot be hooked portably. The user can rebind to any registrable chord
  in Settings.
- **`tauri-plugin-global-shortcut` (RegisterHotKey under the hood), not a raw low-level keyboard
  hook.** Reason:
  `WH_KEYBOARD_LL` hooks are fragile, flagged by AV, and a perf/foreground-window liability. We
  accept `RegisterHotKey`'s constraints (modifier-anchored chords; UIPI limits — see Edge Cases)
  in exchange for stability ([ADR-001](architecture.md), [ADR-011](architecture.md)).
- **Hotkey events emit to the dictation state machine; this module never touches the mic.** It
  emits `Start`/`Stop`/`Cancel` and lets [`dictation.md`](dictation.md) own capture. Keeps the
  trigger pure and unit-testable.

---

## 1. Inputs / Outputs

| Aspect | This feature |
|---|---|
| **Trigger** | Global OS keyboard event for the registered chord, delivered by `tauri-plugin-global-shortcut` even when MIA is unfocused. |
| **Audio in** | N/A — the hotkey module never reads the mic (it only signals [`dictation.md`](dictation.md) to start/stop capture). |
| **Text in** | N/A. |
| **Text out** | N/A — emits **dictation intents** (`Start` / `Stop` / `Cancel`), not text. |
| **Target** | The dictation orchestrator (Rust event bus / Tauri event) and the floating mic HUD (see [tray-and-hud.md](tray-and-hud.md)); the Hub recorder UI when rebinding (see [settings.md](settings.md)). |
| **Language** | Language-agnostic (key events only). |

Backed by the `tauri-plugin-global-shortcut` plugin (`GlobalShortcutExt`, `Shortcut`,
`Modifiers`, `Code`, and the plugin's on-shortcut handler). No audio buffer is touched here, so
**nothing reaches disk** (consistent with ADR-001). The only persisted artifact is the chord/mode
config (see Settings).

---

## 2. Engine Contract (Rust)

Rust is the **engine**; the Svelte UI is a thin webview that only calls typed `invoke()` wrappers
(see [architecture.md](architecture.md)). All commands return `Result<T, String>` — no panics
across the IPC boundary ([ADR-006](architecture.md)).

**Module**: `app/src-tauri/src/hotkey.rs`

```rust
// ---- Config (persisted in settings; see settings.md) ----
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyConfig {
    pub accelerator: String,   // canonical chord string, e.g. "Ctrl+Space" / "Alt+Space"
    pub mode: ActivationMode,  // PushToHold | PressToToggle
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ActivationMode { PushToHold, PressToToggle }

// ---- Intents emitted to the dictation state machine ----
#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum DictationIntent { Start, Stop, Cancel } // emitted via Tauri event "dictation://intent"

// ---- Commands: LIVE (registered in lib.rs; all Result<T,String>, ADR-006) ----
#[tauri::command]
fn register_hotkey(app: AppHandle, rt: State<'_, HotkeyRuntime>, cfg: HotkeyConfig) -> Result<(), String>;
// Parses + validates `cfg.accelerator`, registers via the global-shortcut plugin
// (`GlobalShortcutExt`). Replaces any existing registration atomically
// (unregister-then-register; rolls back on failure).

#[tauri::command]
fn unregister_hotkey(app: AppHandle, rt: State<'_, HotkeyRuntime>) -> Result<(), String>;
// Idempotent: unregistering when nothing is registered is Ok(()).

#[tauri::command]
fn update_hotkey(app: AppHandle, rt: State<'_, HotkeyRuntime>, cfg: HotkeyConfig) -> Result<(), String>;
// = unregister + register; persists cfg only after the OS accepts the new chord.

#[tauri::command]
fn get_hotkey(rt: State<'_, HotkeyRuntime>) -> Result<HotkeyConfig, String>;

// ---- Commands: PHASE-PENDING (specified, NOT yet implemented or registered) ----
// The Settings conflict-probe and the hotkey-recorder ("press a key") capture are
// designed below but are not wired in lib.rs yet. Only register/unregister/update/
// get_hotkey are live today; the recorder reuses get_hotkey and the section below is
// the forward contract.
#[tauri::command]
fn check_hotkey_conflict(accelerator: String) -> Result<ConflictReport, String>; // pending
// Dry-run: parse + probe-register-then-unregister; returns whether the chord is free.

// Hotkey-recorder support (Settings "press a key" capture) — pending:
#[tauri::command]
fn begin_hotkey_capture(state: State<'_, HotkeyState>) -> Result<(), String>; // pending
// Temporarily suspends the live PTT registration so the recorder can read raw keys
// without firing dictation; restores prior registration on end/cancel/timeout.
#[tauri::command]
fn end_hotkey_capture(state: State<'_, HotkeyState>) -> Result<(), String>; // pending

#[derive(Serialize)] // pending — ships with check_hotkey_conflict
#[serde(rename_all = "camelCase")]
pub struct ConflictReport { pub accelerator: String, pub free: bool, pub reason: Option<String> }
```

- **Managed state** (`HotkeyRuntime` in `tauri::State`): the currently registered `Shortcut`, the
  active `HotkeyConfig`, a `pressed: AtomicBool` edge-tracker (for push-to-hold), and a
  `last_event: Instant` for debounce. The `tauri-plugin-global-shortcut` plugin owns the OS
  registration; commands call `app.global_shortcut().register(...)` / `.unregister(...)` through
  `GlobalShortcutExt`, mutating the desired config and (un)registering the `Shortcut`.
- **Event handler**: the plugin invokes the on-shortcut handler (set at build time) on each chord
  press/release; the handler passes the raw edge through the pure `reduce()` reducer (below) and
  emits the resulting `Option<DictationIntent>` to the frontend/orchestrator as the Tauri event
  `"dictation://intent"`.
- **`Err(String)` messages** (each maps to one UI state — see [settings.md](settings.md)):
  - `"empty hotkey"` — no chord given.
  - `"unparseable hotkey: <input>"` — accelerator string didn't parse.
  - `"hotkey must include a modifier"` — bare keys (e.g. `F8` alone, a single letter) are rejected
    in v1 to avoid swallowing normal typing (see Rule 4).
  - `"hotkey already in use by the system or another app: <chord>"` — `RegisterHotKey` returned
    busy.
  - `"Fn key is not hookable"` — user tried to bind a key that resolves to no Windows VK.
- **Pure helpers** (behind `#[cfg(test)]`, no I/O):
  - `parse_accelerator(&str) -> Result<(Modifiers, Code), String>` — canonical chord parser.
  - `to_canonical(Modifiers, Code) -> String` — round-trips a recorder capture back to a string.
  - `reduce(state: EdgeState, ev: RawHotkeyEvent, mode: ActivationMode, now: Instant) -> (EdgeState, Option<DictationIntent>)`
    — the **debounce + mode** reducer; the heart of the testable logic.
  - `is_bare_key(&Modifiers) -> bool`, `is_reserved(Modifiers, Code) -> bool` (Win+L, Ctrl+Alt+Del, etc.).
- **Typed UI wrapper**: `app/src/lib/hotkey.ts` exposes the live
  `registerHotkey() / unregisterHotkey() / updateHotkey() / getHotkey()` (`invoke<…>(…)`); the
  `checkHotkeyConflict() / beginHotkeyCapture() / endHotkeyCapture()` wrappers ship with the
  Phase-pending recorder/conflict-probe commands above. The UI holds **no** trigger logic.

---

## 3. Business Rules

1. **Single global registration.** Exactly one PTT chord is registered at a time. `update_hotkey`
   must unregister the old chord before the new one is live; if the new registration fails, the old
   one is restored and the config is **not** persisted (no "dead" hotkey state).
2. **Unfocused operation.** The hotkey must fire when any other application is foreground; MIA need
   not be visible or focused. (This is the whole point of a *global* hotkey.)
3. **push-to-hold semantics.** On the chord's key-**down** edge → emit `Start`. On the **release**
   of any key in the chord (i.e. the chord is no longer fully held) → emit `Stop`. Auto-repeat
   key-down events while held are **ignored** (no repeated `Start`).
4. **press-to-toggle semantics.** A complete press-and-release of the chord toggles: first
   activation → `Start`; next activation → `Stop`. While *toggled on*, the key need not stay held.
5. **Modifier-anchored chords only (v1).** A registrable chord must include at least one modifier
   (`Ctrl`/`Alt`/`Shift`/`Win`). Bare keys are rejected (`"hotkey must include a modifier"`) so the
   trigger never eats ordinary typing. (`tauri-plugin-global-shortcut` can register some bare
   F-keys, but we deliberately forbid it in v1.)
6. **`Fn` and non-VK keys are rejected** with `"Fn key is not hookable"`. The recorder must not let
   the user "save" a capture that produced no Windows virtual-key.
7. **Reserved system chords are rejected.** OS-owned chords (`Win+L`, `Ctrl+Alt+Del`, `Win+Tab`,
   `Alt+Tab`, `Win+D`, etc.) cannot be claimed; `check_hotkey_conflict`/`register_hotkey` return
   the busy/reason error and the recorder refuses the binding.
8. **Conflict detection is a real probe.** `check_hotkey_conflict` actually attempts an OS
   registration (then immediately unregisters) rather than guessing from a static list — this is the
   only reliable way to know whether *another running app* already grabbed the chord. Static reserved
   chords (Rule 7) are checked first to short-circuit.
9. **Debounce.** Events within a `debounce_ms` window (default **40 ms**) of the last accepted edge
   for the *same* logical transition are dropped. This collapses key chatter and OS auto-repeat into
   a single intent (see Rule 3).
10. **Rapid re-trigger guard.** A `Start` is ignored if dictation is already active (the reducer
    tracks `active`); a `Stop`/`Cancel` is ignored if not active. The hotkey module never emits a
    second `Start` before a `Stop` — re-entry protection lives here so [`dictation.md`](dictation.md)
    sees a clean alternating stream.
11. **Lost release ⇒ safety `Stop`.** In `push-to-hold`, if the key-up edge is never delivered (e.g.
    focus changed to a higher-integrity window mid-hold — see Edge Cases), a *missing-release watchdog*
    times out at `max_hold_ms` (default **30 s**) and emits `Stop`, so dictation can't get stuck "on".
12. **Capture mode suspends live PTT.** While the Settings recorder is capturing
    (`begin_hotkey_capture`), the live hotkey is unregistered so pressing keys to *record* a new
    chord does not start dictation. `end_hotkey_capture` (or a 15 s capture timeout) restores the
    previous registration.
13. **`Esc` cancels, in both modes.** While dictation is active, the `Esc` key (registered as a
    secondary, transient hotkey only during an active session) emits `Cancel` (discard, no
    injection). This is fixed, not rebindable, in v1.
14. **Persisted, validated config.** The chord + mode persist via Settings ([settings.md](settings.md)).
    On app start, the saved chord is re-registered; if it now conflicts (another app took it),
    MIA surfaces a non-blocking warning and leaves the hotkey *unregistered* until resolved (it does
    **not** silently fall back to the default and steal a key the user didn't choose).
15. **Self-healing registration (no dead hotkey until restart).** Windows can silently drop a
    `RegisterHotKey` routing while MIA keeps running — an IME/TSF arming `Ctrl+Space` (the chord is
    also the East-Asian input-method toggle), a sleep/resume or lock/unlock transition, or another
    app briefly grabbing the chord. The single startup registration (Rule 14) has no recovery, so
    such a loss would otherwise persist until the app is restarted. MIA therefore re-claims the chord
    automatically: (a) an **idle self-heal tick** re-registers it every `self_heal_interval_ms`
    (default **30 s**) while no session is active — never mid-hold; (b) a **resume/unlock watcher**
    (Windows power-broadcast + WTS session-unlock) re-registers it **immediately** on return; and
    (c) regaining focus on the Hub window re-registers it. A tray item (**"Reativar atalho"**) does
    the same on demand, replacing the close-and-reopen workaround. Re-registration is idempotent
    (unregister-then-register: a healthy binding is a no-op, a dropped one is restored) and resets the
    edge tracker. It runs **off** the plugin's shortcut-handler thread and **off** the main loop
    (calling the plugin's register/unregister on either would deadlock — see §5). It does **not**
    durably defeat a still-armed IME (that needs a different chord, Rule 7/8); it cures a *dropped*
    routing, which is the restart-fixes-it case.

---

## 4. Options & Defaults

| Option | Type | Range / values | Default | Effect |
|---|---|---|---|---|
| `accelerator` | string (canonical chord) | any registrable modifier-anchored chord (with a non-modifier key) | `"Ctrl+Space"` | The global PTT trigger. |
| `mode` | enum | `pushToHold` \| `pressToToggle` | `pushToHold` | Hold-to-dictate vs. press-to-start/stop. |
| `debounce_ms` | int (ms) | 0–200 | `40` | Window that collapses key chatter/auto-repeat (Rule 9). Advanced; rarely changed. |
| `max_hold_ms` | int (ms) | 5 000–120 000 | `30 000` | Missing-release watchdog timeout (Rule 11). |
| `capture_timeout_ms` | int (ms) | fixed | `15 000` | Recorder auto-cancel (Rule 12). Not user-facing. |
| `cancel_key` | (fixed) | `Esc` | `Esc` | Cancel-active-dictation key (Rule 13). Fixed in v1. |
| `self_heal_interval_ms` | int (ms) | fixed | `30 000` | Idle re-registration cadence (Rule 15). Re-claims a silently-dropped chord; skipped while a session is active. Not user-facing. |

Validation: the Settings recorder disables **Save** for bare keys (Rule 5), `Fn`/non-VK captures
(Rule 6), and reserved/already-taken chords (Rules 7–8). The engine **re-validates** every value
defensively in `register_hotkey` (the UI guard is convenience, not the source of truth — ADR-006).

---

## 5. Threading / Performance

- **Plugin-owned registration.** The OS registration is owned by the `tauri-plugin-global-shortcut`
  plugin (Windows `RegisterHotKey` posts `WM_HOTKEY` to a thread's message queue); the plugin
  dispatches the on-shortcut handler. Commands invoked from the UI mutate desired config and call the
  plugin's `register`/`unregister` through `GlobalShortcutExt`.
- **No audio here.** This module does not touch cpal or the warm STT model — it only emits intents.
  The audio thread and the **warm/resident** `whisper-server` sidecar ([ADR-004](architecture.md))
  are owned by [`dictation.md`](dictation.md)/[`speech-to-text.md`](speech-to-text.md). This feature
  does **not** cold-spawn a per-utterance CLI.
- **Latency budget.** The hotkey path is effectively free: key-down → `reduce()` → emit `Start` is
  microseconds. The perceived "press → listening" latency is dominated by [`dictation.md`](dictation.md)
  starting the (already-open, warm) audio stream, not by this module. Target: HUD shows *Listening*
  **< 50 ms** after key-down.
- **Cancellation.** `Esc`/`Cancel` (Rule 13) and the `max_hold_ms` watchdog (Rule 11) both emit a
  terminal intent; the actual capture teardown / cancel-flag is handled downstream. No in-flight STT
  work is owned here.
- **Resource use.** Negligible — one OS hotkey registration, one atomic edge-tracker, one timer for
  the watchdog. No model RAM.
- **Self-heal threading (Rule 15).** Re-registration (`do_reregister`) calls the plugin's
  `register`/`unregister`, which post to the main loop and **block** until it runs them — so calling
  them from the plugin's shortcut-handler thread *or* from the main loop (tray/window-event callbacks)
  would deadlock. The idle tick owns a dedicated thread; the on-demand callers (`request_reregister`)
  each spawn a short-lived worker. The resume/unlock watcher owns a hidden top-level window on its own
  thread (top-level so it receives `WM_POWERBROADCAST`; `WTSRegisterSessionNotification` for unlock),
  failing safe to the idle tick if window creation fails. A skipped/extra tick on a wall-clock jump is
  harmless (idempotent re-register).

---

## 6. UI States

This feature has **two** surfaces: it *drives* the floating mic HUD's state machine (indirectly,
via the intents it emits) and it *owns* the **hotkey-recorder** UI in the Settings/Hub window.

```
Intent stream → dictation state machine (owned by dictation.md / HUD):
  Idle(hidden) → [Start] → Listening(pulsing waveform) → [Stop] → Transcribing(spinner)
              → Inserting(brief check) → Idle | [Cancel/Esc] → Idle | Error(message)

Hotkey-recorder (Settings/Hub, light theme):
  Idle("Ctrl+Space — Click to change")
    → Capturing("Press a key combination…")   // begin_hotkey_capture
    → Captured(chord)                          // valid, conflict-free → enable Save
    → Conflict(chord, reason)                  // taken/reserved/bare/Fn → show why, disable Save
    → (Save → update_hotkey → Idle) | (Esc/timeout → Idle, restore prior)
```

- **HUD** (while dictating): this module triggers the *transitions*; the per-state visuals
  (waveform level meter, spinner, check, error) and the single action-blue accent live in
  [tray-and-hud.md](tray-and-hud.md) / [design-system.md](design-system.md).
- **Settings/Hub recorder**: a `Field` with a "Recording…" `Pill`, a live-rendered chord, an inline
  validation message, and the mode toggle (`Toggle`/segmented control: *Hold* vs. *Toggle*). Empty
  state shows the current binding; error/conflict shows the reason text (don't rely on color alone).
- Keep the one-action-color discipline; recorder controls and hit targets ≥ 40px.

---

## 7. Edge Cases

| Scenario | Expected behavior |
|---|---|
| Chord already taken by the OS or another running app | `register_hotkey` / `check_hotkey_conflict` returns `"hotkey already in use…"`; recorder shows the reason and disables **Save** (Rules 7–8). |
| User binds a reserved system chord (`Win+L`, `Alt+Tab`, …) | Rejected before probing via the static reserved-list (Rule 7). |
| User presses `Fn` (or a key with no Windows VK) in the recorder | Capture is discarded; `"Fn key is not hookable"`; Save stays disabled (Rule 6). |
| Bare key (single letter / lone `F8`) | Rejected `"hotkey must include a modifier"` (Rule 5). |
| Key **held across a focus change** to a higher-integrity (elevated/UAC) window | The key-up may not be delivered (UIPI / `RegisterHotKey` scope). The `max_hold_ms` watchdog emits a safety `Stop` (Rule 11); dictation never sticks "on". |
| OS auto-repeat fires many key-down events while held | Collapsed to a single `Start` by the auto-repeat filter + debounce (Rules 3, 9). |
| Rapid re-trigger (press-press-press faster than `debounce_ms`) | Extra edges dropped; re-entry guard prevents a second `Start` before a `Stop` (Rules 9–10). |
| Saved chord conflicts on next app start (another app grabbed it) | Non-blocking warning in the Hub; hotkey left **unregistered**, default is **not** silently substituted (Rule 14). |
| Recorder open while the live PTT is registered | Live PTT is suspended during capture so recording keys doesn't start dictation; restored on end/cancel/timeout (Rule 12). |
| MIA not elevated, target window is elevated | Hotkey may still not reach an elevated foreground app, and injection downstream can fail silently ([ADR-005](architecture.md)) — surfaced by the HUD, not this module. |
| `Esc` pressed mid-dictation | `Cancel` emitted; capture discarded, nothing injected (Rule 13). |
| PTT chord silently stops firing while the app keeps running (dropped `RegisterHotKey` routing — IME/TSF, sleep/lock, brief grab) | Self-heal re-claims it: idle tick (≤ `self_heal_interval_ms`), or immediately on resume/unlock / Hub focus, or via the tray **"Reativar atalho"** item — no restart needed (Rule 15). |
| Self-heal tick fires while a push-to-hold session is active | Skipped that cycle (never yank the chord mid-hold); retried once idle (Rule 15). |
| Chord currently held by another app/IME when self-heal runs | Re-register fails (`AlreadyRegistered`), left as-is; the next tick re-claims it once the other owner releases it (Rule 15). |

---

## 8. Testing Checklist

- **Rust** (`cargo test`, no I/O — pure helpers only):
  - [x] `parse_accelerator` accepts canonical chords (`"Ctrl+Space"`, `"Alt+Space"`,
        `"Ctrl+Shift+D"`) and rejects empty / unparseable / bare-key / `Fn` inputs with the exact
        `Err(String)` messages.
  - [x] `to_canonical` round-trips: `parse_accelerator(to_canonical(m, c)) == (m, c)`.
  - [x] `reduce()` in `PushToHold`: down → `Some(Start)`; auto-repeat downs → `None`; release →
        `Some(Stop)`; release-when-inactive → `None`.
  - [x] `reduce()` in `PressToToggle`: 1st complete press → `Start`; 2nd → `Stop`; alternation holds.
  - [x] Debounce: two edges within `debounce_ms` collapse to one accepted intent.
  - [x] Re-entry guard: no second `Start` before a `Stop`; no `Stop` while inactive.
  - [x] `is_reserved` flags `Win+L`, `Alt+Tab`, `Ctrl+Alt+Del`, etc.
- **Manual / runtime** (needs a real desktop + another focused app):
  - [ ] PTT fires while a *different* app (browser, editor) is focused (Rule 2).
  - [ ] `push-to-hold`: HUD goes *Listening* on press, *Transcribing* on release.
  - [ ] `press-to-toggle`: press starts, press again stops; works without holding.
  - [ ] Recorder: capture a new chord, conflict warning on a taken chord, Save persists & re-registers.
  - [ ] Hold across an elevated-window focus change → watchdog `Stop` fires (no stuck session).
  - [ ] `Esc` mid-dictation cancels with no injection.
  - [ ] Restart with a now-conflicting saved chord → warning, hotkey unregistered (Rule 14).

---

## 9. Out of Scope (this version)

- **Configurable multi-action keymap** (separate keys for toggle / Command Mode / Polish) —
  Phase 2 reuses the *same* trigger and routes by intent; see [ai-commands.md](ai-commands.md).
- **Per-app hotkey profiles** — deferred to Phase 3 personalization ([../ROADMAP.md](../ROADMAP.md)).
- **Low-level keyboard hook (`WH_KEYBOARD_LL`)** to support bare keys / chord-less F-keys / true `Fn`
  capture — deliberately avoided for stability (see Scope decisions); revisit only if real demand.
- **Mouse-button or pedal triggers, double-tap modifier triggers** — Backlog (Phase 5).
- **macOS / Linux hotkey backends** (Carbon/CGEvent, Wayland) — deferred with the platform itself
  ([ADR-011](architecture.md), Phase 5 / Backlog).
