# Tray & Recording Indicator Feature Spec

> **⚠️ DESIGN CHANGE (2026-06-04): the recording indicator is now USER-SELECTABLE.** A new setting
> `hud.indicator` (enum `overlay` | `tray` | `both`, **default `both`**) chooses which surface
> shows the live phase, exposed in the **Ditado** view. Both surfaces are live:
> - **Overlay** — the floating mic HUD window (`hud.rs`, `HudWindow.svelte`, `MicHud.svelte`, the
>   `"hud"` window in `tauri.conf.json`), driven by the engine's `hud://state` + `hud://level` events
>   (unchanged from the original design — Sections 3/6/7/8 still apply to it).
> - **Tray** — `tray::reflect_phase` paints a colored corner badge (red = listening, pumpkin/amber =
>   transcribing/inserting) onto the brand icon and updates the tooltip per phase.
>
> The engine's `dictation.rs::show_phase` reads the setting per phase-change and drives the overlay
> (`hud://state`), the tray badge, or both. The capture thread streams the HUD waveform (`hud://level`)
> only when the overlay is active (tray-only dictation needs no waveform).
>
> **Status**: Phase 1 — **tray + HUD window both implemented and validated on Windows**. The tray
> (`app/src-tauri/src/tray.rs`: Open Settings/Hub + Reativar atalho + Quit) is live and now doubles as
> a selectable recording indicator. **Close-to-tray is wired**: closing the Hub hides it to the tray
> (`lib.rs` `on_window_event` → `prevent_close` + `hide`) instead of quitting; only the tray "Sair"
> exits. The richer tray menu (a "pick model" submenu) remains the documented Phase-1 backlog.
> **Last updated**: 2026-06-04
> **Coverage**: Sections 1–9 drafted (tray + HUD as one feature; two surfaces). §2's exact command set
> is partially superseded by the event-driven `hud.rs` + `dictation.rs` implementation, plus the new
> `tray::reflect_phase` tray-badge path gated by `hud.indicator`.
> **Environment**: desktop (Windows, native)

MIA has **no main window** — it lives in the **system tray** and surfaces a tiny **floating mic
HUD** only while you dictate. This spec covers both halves of MIA's ambient presence: (1) the
**system tray** (Tauri's built-in **tray-icon feature**, driven by `tray.rs` — *not* a standalone
`tray-icon` crate) as the app's primary, always-present face — a tray icon whose art reflects state
(idle / listening / disabled) and a menu to open Settings/Hub, toggle dictation on/off, pick the
active model, and quit; and (2) the **floating mic HUD** (`MicHud.svelte` in a
frameless, transparent, always-on-top, no-activate, click-through Tauri window) that fades in on
hotkey-down and walks the `listening → transcribing → inserting → error` state machine, anchored
near the caret when possible and screen-anchored otherwise. The tray is up the whole time MIA
runs; the HUD is up only during an utterance. Both are **thin views** driven by state pushed from
Rust — neither holds dictation logic (see [architecture.md](architecture.md)). This is the
**presentation layer** of the dictation pipeline (hotkey → capture → VAD → STT → cleanup → inject):
the tray triggers/gates it and the HUD reflects it. Lands in **Phase 1 — Core Dictation MVP**
(see [../ROADMAP.md](../ROADMAP.md)); implements **ADR-001** (privacy-first, tray-resident, no
account/window), **ADR-002** (Tauri 2 + Svelte 5 + WebView2), **ADR-005** (the HUD must never
break SendInput injection by stealing focus), and **ADR-006** (`Result<T, String>` IPC).

**Scope decisions** (locked at design time):

- **Tray is the primary presence; no main window on launch.** MIA boots straight to the tray with
  the HUD hidden — there is no default window to focus or close. Closing the Hub hides it back to
  the tray; only the tray "Quit" item exits the process (ADR-001 — a resident, ambient tool /
  Phase 1).
- **HUD is a separate, dedicated Tauri window, not an element inside the Hub.** It is frameless,
  transparent, `skip_taskbar`, always-on-top, and **no-activate** so it can float over arbitrary
  apps without becoming a normal window (ADR-002 / Phase 1).
- **The HUD never takes focus — non-negotiable.** Synthetic `SendInput` lands in the *focused*
  window; if the HUD activates, the caret moves to the HUD and injection breaks (ADR-005). Every
  HUD rule below serves this constraint (Phase 1).
- **The HUD is display-only — pushed state, no input.** It renders a single state value streamed
  from Rust and exposes **no clickable controls** in V1 (clicking would risk activation). All
  controls live in the tray menu and the Hub (Phase 1).
- **One Blush language, two surfaces.** The HUD is a white outlined pill (solid white, 2px charcoal
  outline, pumpkin waveform) — no dark theme and no `hud-*` tokens; the Hub is the blush canvas (see
  [design-system.md](design-system.md) §2 / Phase 1).
- **Caret-anchored when discoverable, screen-anchored otherwise.** Position priority is
  near-caret → fixed screen anchor (default bottom-center); the user can force a fixed anchor in
  [settings.md](settings.md) (Phase 1).
- **No audio, no STT, no injection in this module.** `tray.rs` / `hud.rs` only own the tray, the
  HUD window, and the state/level events they render; the pipeline lives in
  [dictation.md](dictation.md) (Phase 1).

---

## 1. Inputs / Outputs

This feature is **presentation + control surface**, not part of the audio path. It consumes the
dictation state machine and the live mic level meter, and it emits user intents (toggle, pick
model, open Hub, quit).

| Aspect | This feature |
|---|---|
| **Trigger** | Tray icon click / tray menu selection; **state events** from the dictation orchestrator ([dictation.md](dictation.md)) that drive the tray icon art and the HUD state. The HUD itself never *starts* dictation — the global PTT hotkey does ([hotkeys.md](hotkeys.md)). |
| **Audio in** | N/A directly. Receives a **derived RMS/level value** (a single `f32` per frame, already computed off the cpal callback — see [audio-capture.md](audio-capture.md)) to animate the waveform. Raw PCM never reaches this module. |
| **Text in** | N/A. The HUD shows fixed per-state labels ("Listening…", "Transcribing…", "Inserted", error text), never the transcript itself in V1. |
| **Text out** | N/A. No injection here. Emits **intents**: active-model selection, "open Hub", "quit" — persisted via [settings.md](settings.md). |
| **Target** | The OS tray notification area; the dedicated frameless HUD window (always-on-top overlay); the Hub window (shown/hidden). |
| **Language** | UI labels are localized (pt-BR / English, first-class — see [design-system.md](design-system.md)); the feature itself is language-agnostic. |

Crates / features: Tauri's built-in **tray-icon feature** (`tauri = { features = ["tray-icon"] }`,
driven by `tray.rs` — there is no separate `tray-icon` crate) for the tray + menu; Tauri's
**`WebviewWindowBuilder`** / `tauri-plugin-positioner` (the planned `hud.rs` window + positioning,
not yet implemented — see §2), `tauri::Manager`/`Emitter` (state push).
No audio buffer ever touches disk in this module (ADR-001) — it never even sees the buffer, only a
scalar level.

---

## 2. Engine Contract (Rust)

> ✅ **CURRENT (2026-06-04) — selectable indicator: overlay, tray, or both.** The engine reflects the
> dictation phase on whichever surface(s) the user chose (`hud.indicator`). The HUD overlay is driven
> by the global `hud://state`/`hud://level` events (as originally designed); the tray badge is driven
> by a single internal helper (no `#[tauri::command]`, no IPC — engine and tray are both in-process):
>
> ```rust
> // app/src-tauri/src/dictation.rs — the dispatcher, called on every phase change
> fn show_phase(app: &AppHandle, phase: Phase, message: Option<&str>);  // overlay and/or tray
>
> // app/src-tauri/src/tray.rs — the tray-badge half
> pub fn reflect_phase(app: &AppHandle, phase: Phase, message: Option<&str>);
> // Pure, cargo-tested helpers behind it:
> fn phase_tooltip(phase: Phase, message: Option<&str>) -> String;  // pt-BR tooltip per phase
> fn phase_badge(phase: Phase) -> Option<[u8; 3]>;                  // badge color, None = plain icon
> fn overlay_badge(rgba: &mut [u8], w: u32, h: u32, rgb: [u8; 3]);  // paint a corner dot in place
> ```
>
> Tray icon (the brand icon with a dot painted over it at runtime — `overlay_dot`, no asset):
> **Listening** → a **big red ball** (`#E53E3E`) with a soft red glow halo in the top-right corner
> + "MIA — ouvindo…";
> **Transcribing/Inserting** → a smaller amber (`#F2A033`) dot, bottom-right + "MIA —
> transcrevendo…/inserindo…"; **Idle** → plain icon + "MIA — ditado local"; **Error** (transient) →
> plain icon + message in the tooltip ("MIA — erro: …").
>
> The `show_hud`/`hide_hud`/`open_hub` commands below remain the **unimplemented** original design,
> kept for history; the overlay is wired via events, not those commands.

> ⚠️ **HISTORICAL / PARTIALLY-SUPERSEDED — richer commands not wired.** The commands in this section
> (`show_hud` / `hide_hud` / `set_active_model` / `open_hub`) are the
> **planned target** and are **not yet implemented** — none are registered in `lib.rs`'s
> `invoke_handler`. **What exists today:** `app/src-tauri/src/tray.rs` implements the system tray
> via Tauri's built-in tray-icon feature, with **Open Settings/Hub**, **Reativar atalho**, and **Quit** menu items; and
> `app/src-tauri/src/hud.rs` exists and is wired at startup (`hud::setup_hud` in `lib.rs`) doing the
> native window plumbing — click-through (`set_ignore_cursor_events`) + bottom-center docking
> (`dock_bottom_center`). The mic HUD is a **dedicated Tauri window** labeled `"hud"` rendering
> `HudWindow.svelte`, driven by `hud://state` events emitted from `dictation.rs`. The richer tray
> menu ("Pick model" submenu) and the richer HUD commands above
> remain on the Phase-1 backlog. Treat those signatures as the design contract to build against, not
> as the live IPC surface.

The intended design: Rust owns the tray and the HUD window; Svelte renders `MicHud.svelte` and the
menu has no webview at all (native menu). All commands return `Result<T, String>` (ADR-006).

**Modules (planned)**: `app/src-tauri/src/tray.rs` (tray icon + menu — *implemented today, Open +
Quit only*) and `app/src-tauri/src/hud.rs` (HUD window lifecycle + positioning — *not yet created;
HUD is currently the `MicHud.svelte` overlay*). Shared state lives in the app's managed `State`.

```rust
// ⚠️ PHASE-PENDING: the HudState enum and all commands below are PLANNED — none are implemented
//    or registered in lib.rs yet. See the callout above.
// ---- HUD state pushed Rust → UI (the HUD is display-only) ----
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
enum HudState {
    Idle,                                  // window hidden
    Listening { level: f32 },              // 0.0..=1.0 mic RMS for the waveform
    Transcribing,                          // whisper running on the buffer
    Inserting,                             // brief success tick (~400 ms)
    Error { message: String },            // short label; details go to the Hub
}

// ---- HUD window management (called by the dictation orchestrator, not the user) ----
#[tauri::command]
async fn show_hud(app: AppHandle, near_caret: bool) -> Result<(), String>;
#[tauri::command]
async fn hide_hud(app: AppHandle) -> Result<(), String>;
// State is streamed by emitting the GLOBAL events `hud://state` (HudState) and `hud://level`
// (f32) via app.emit(…); the HUD window (label "hud", HUD_LABEL in hud.rs; rendered by
// HudWindow.svelte) listens for them. The level meter uses a throttled event, NOT a command.

// ---- Tray-driven intents (also reachable from the Hub) ----
#[tauri::command]
async fn set_active_model(state: State<'_, AppState>, model_id: String) -> Result<(), String>;
#[tauri::command]
async fn open_hub(app: AppHandle) -> Result<(), String>;   // show + focus the Hub window
// Quit is a native menu item handled in the tray event loop (graceful shutdown), not a command.
```

- `HudState` is `serde(rename_all = "camelCase")`; the UI matches on `kind`. `level` is clamped
  `0.0..=1.0`. `Error.message` is a short user-presentable string (the long detail is logged and
  shown in the Hub).
- **`Err(String)` paths**: `show_hud` → `Err("failed to create HUD window: …")` if window
  creation fails; `set_active_model` → `Err("model not downloaded")` (gate to
  [speech-to-text.md](speech-to-text.md)) or `Err("model swap failed: …")`; `open_hub` →
  `Err("hub window unavailable")`. The tray menu re-checks these defensively (a stale "pick model"
  click can't crash anything).
- **Native, no sidecar.** The tray (Tauri's built-in tray-icon feature) and the planned HUD
  `WebviewWindowBuilder` are in-process; no external binary. The HUD window is intended to be built
  once at startup (hidden) and shown/hidden thereafter — recreating it per utterance would add
  latency and flicker. (Today's `MicHud.svelte` overlay is the interim stand-in for that window.)
- **Pure helpers (cargo-tested, no I/O)** — list:
  - `tray_menu_model() -> Menu` builder from the model registry (mirrors Toolzy's `MODELS`) — the
    "pick model" submenu, with the active model checkmarked.
  - `tray_icon_for(state: TrayVisualState) -> &'static [u8]` — maps idle/listening/disabled to the
    embedded icon bytes (pure mapping).
  - `hud_anchor(monitor: MonitorRect, caret: Option<Point>, pref: AnchorPref) -> Point` — the
    positioning math (near-caret vs screen anchor; clamps the pill fully on-screen). The hard part
    is pure and testable.
  - `level_to_bars(level: f32, n: usize) -> Vec<f32>` — maps an RMS level to N waveform bar
    heights (also usable UI-side; the canonical version is here for tests).
- **Typed UI wrappers (planned)** in `app/src/lib/hud.ts` (`onHudState`, `onHudLevel` event
  listeners) and `app/src/lib/tray.ts` (`setDictationEnabled`, `setActiveModel`, `openHub`) — one
  per group; the UI holds **no** dictation logic. *Not yet created* — they land with the `hud.rs`
  window and the richer tray menu; the current `MicHud.svelte` overlay does not yet consume them.

---

## 3. Business Rules

1. **The HUD never takes focus.** The HUD window is created **no-activate** (Win32
   `WS_EX_NOACTIVATE`) + always-on-top + `skip_taskbar`; showing it, moving it, or updating it
   must **never** call `set_focus`, `set_focusable(true)`, or anything that activates it. Test:
   after `show_hud`, the previously-focused window remains the foreground/active window. This is
   the rule the whole feature exists to protect (ADR-005) — if it breaks, injection breaks.
2. **HUD is click-through where the OS allows.** The window is made transparent to mouse input
   (Win32 `WS_EX_TRANSPARENT` / `set_ignore_cursor_events(true)`) so a click "through" the pill
   lands in the app underneath; the HUD never blocks the target app. If the OS refuses
   click-through, the HUD still must not steal focus (Rule 1 still holds).
3. **HUD shown only during an utterance.** `Idle ⇒ window hidden` (not a resting pill). It fades in
   on `Listening` and fades out after `Inserting`/`Error`. There is no persistent overlay
   (design-system §9a principle 3).
4. **HUD mirrors the dictation state machine exactly.** Its state is a 1:1 view of the orchestrator
   state ([dictation.md](dictation.md)); the HUD derives nothing on its own and shows whatever
   `hud://state` last said. A late/dropped event must never leave the HUD stuck visible — a hide is
   idempotent and a watchdog hides it if no event arrives within a timeout (see Rule 12).
5. **Tray icon reflects state.** Two visuals: **idle** (MIA enabled, not dictating) and
   **listening** (an utterance is active — accent dot/ring). The icon updates on enter/leave of an
   utterance.
7. **"Pick model" reflects and changes the active model.** The tray model submenu lists downloaded
   models with the active one checkmarked; selecting another calls `set_active_model`, which
   triggers a **warm-model swap** (ADR-004) — the tray shows a transient "loading model" hint and
   the next utterance uses it. Selecting a **not-yet-downloaded** model returns
   `Err("model not downloaded")` and opens the Hub's download gate
   ([speech-to-text.md](speech-to-text.md)) rather than failing silently.
8. **"Open Settings/Hub" shows and focuses the Hub.** `open_hub` creates-or-shows the single Hub
   window and focuses **it** (the Hub *is* allowed to take focus — it's a normal window, unlike the
   HUD). Closing the Hub **hides** it (back to tray), it does not quit.
9. **Only "Quit" exits.** The tray "Quit" item performs a graceful shutdown: unregister the global
   hotkey, stop any in-flight capture/cancel STT, unload the warm model, restore the clipboard if a
   paste-fallback was mid-flight (ADR-005), then exit. No other action terminates the process.
10. **Single-instance.** A second launch must not spawn a second tray icon / second HUD; it
    focuses the existing instance's Hub (or no-ops) — MIA is a single resident process.
11. **HUD position is computed at show-time, per active monitor.** On `show_hud`, position via
    `hud_anchor`: near the caret if its screen point is known, else the configured screen anchor
    (default bottom-center of the monitor containing the foreground window). The pill is clamped to
    stay fully on the visible work area (never under the taskbar, never off-screen) — see Rule 14.
12. **Stuck-HUD watchdog.** If the HUD is visible and no `hud://state` update arrives within a
    bounded window (e.g. dictation crashed/cancelled), the HUD auto-hides and the tray returns to
    idle. The HUD must never be left orphaned over the user's screen.
13. **Reduced-motion honored.** When `prefers-reduced-motion` is set, the waveform/pulse becomes a
    static pumpkin "listening" dot and the spinner becomes a non-spinning indicator
    (design-system §9c). State labels still change so the state is never conveyed by motion alone.
14. **Multi-monitor & DPI correct.** Positioning uses the monitor under the **foreground window**
    (or the cursor as fallback) and that monitor's scale factor; the HUD appears on the screen the
    user is working on, not always the primary, and is sized in logical pixels so it's the same
    physical size across mixed-DPI monitors.
15. **Fullscreen / exclusive apps.** Over a normal full-screen window the always-on-top HUD still
    draws; over a true exclusive-fullscreen app (some games) the OS may suppress overlays — MIA does
    not fight the compositor. Dictation still works (text still injects); only the HUD may be hidden
    by the foreground app, which is acceptable (the feedback is non-essential to injection).
16. **Error states auto-dismiss but persist in the Hub.** A `HudState::Error` shows for a few
    seconds then fades; the full message is recorded for the Hub's status area so the user can read
    it after it's gone (don't rely on the transient HUD alone).

---

## 4. Options & Defaults

| Option | Type | Range / values | Default | Effect |
|---|---|---|---|---|
| `hudEnabled` | bool | on / off | `true` | If off, dictation still works but no HUD is shown (tray icon still reflects state). |
| `hudAnchor` | enum | `caret` · `bottomCenter` · `bottomRight` · `topCenter` | `caret` | Position strategy; `caret` falls back to `bottomCenter` when the caret point is unknown (Rule 11). |
| `hudClickThrough` | bool | on / off | `true` | Make the pill mouse-transparent where the OS allows (Rule 2). Off keeps no-activate but lets the pill be hovered. |
| `hudShowLanguageTag` | bool | on / off | `false` | Show the detected language tag (dimmed charcoal) on the pill. |
| `startMinimizedToTray` | bool | on / off | `true` | Launch to tray with no window (the V1 default presence). |
| `playSoundOnStart` | bool | on / off | `false` | Optional cue on listening-start (unobtrusive-by-default — design-system §9a). |
| `activeModel` | enum | downloaded model ids | smallest downloaded | The warm STT model; also settable from the tray submenu (Rule 7 / [speech-to-text.md](speech-to-text.md)). |

The HUD's per-state visuals, the one-action-color discipline, fade timing, and tokens are **fixed
by [design-system.md](design-system.md)**, not user options. STT anti-hallucination defaults are
fixed elsewhere (ADR-007) and are not exposed here.

---

## 5. Threading / Performance

This is the **feedback** surface, so it must be cheap and never on the hot path to injected text.

- **No audio-thread work.** This module never touches the cpal callback. The waveform is fed by a
  **throttled, pre-derived level value** (one `f32`, computed off-thread — see
  [audio-capture.md](audio-capture.md)), emitted to the HUD window at a modest rate (e.g. ~30–60
  Hz, coalesced) so the WebView2 animation stays smooth without flooding IPC.
- **No model work here.** Showing the HUD and updating the tray icon must happen **instantly** on
  hotkey-down — *before* any STT — so `Listening` is visible immediately (design-system §9a
  principle 1). The warm model (ADR-004) is owned by [speech-to-text.md](speech-to-text.md); a
  model **swap** triggered from the tray (Rule 7) runs off the UI/command thread and reports via a
  `Channel`; this module does **not** cold-spawn `whisper-cli`.
- **HUD window is built once, reused.** Created hidden at startup; `show_hud`/`hide_hud` only
  toggle visibility + position + opacity (fade). Avoid create/destroy per utterance (latency +
  flicker + a focus-steal risk on creation).
- **Latency budget**: hotkey-down → HUD `Listening` visible target **< 50 ms** (pure window show +
  one event); this is independent of and must never gate the STT path. The dominant cost in the
  overall flow is STT inference, which is entirely outside this module.
- **Cancellation**: on hotkey release / abort / disable, the orchestrator drives the HUD to
  `Transcribing`/`Inserting` or straight to hidden; the watchdog (Rule 12) is the backstop. The
  tray "Quit" path cancels in-flight work via the managed cancel flag (Toolzy's cancel pattern)
  before exit (Rule 9). No partial transcript is owned here.
- **Resource use**: negligible — a small always-on-top WebView and a tray icon. No model RAM is
  attributed to this module.

---

## 6. UI States

Two surfaces. The **HUD** owns the dictation feedback state machine (a white Blush pill — solid
white, 2px charcoal outline); the **tray icon** is a 3-value reflection of it; the **Hub** (the
blush canvas) is shown/hidden by the tray.

```
HUD state machine (mirrors dictation orchestrator — see dictation.md):

  Idle(window hidden)
     │  hotkey down  &&  hudEnabled   → show_hud (no-activate, positioned)
     ▼
  Listening(pumpkin waveform reacting to the level meter, on the white pill)
     │  endpoint / hotkey release
     ▼
  Transcribing(pumpkin spinner; waveform frozen/dimmed; "Transcribing…")
     │  text injected (handled by text-injection.md)
     ▼
  Inserting(brief success-token ✓ tick ~400 ms; "Inserted")
     │  fade out
     ▼
  Idle(window hidden)

  Any state ──(mic lost / no speech / STT fail / injection blocked)──► Error(danger-token ⚠ + label)
                                                                        → auto-dismiss → Idle
```

- **HUD** (`MicHud.svelte`, a white pill with a 2px charcoal outline — design-system §7 "Mic HUD pill"):
  - `listening`: pumpkin waveform bars reacting to the level meter; charcoal label "Listening…".
  - `transcribing`: small pumpkin spinner, waveform dimmed; charcoal label "Transcribing…".
  - `inserting`: success-token ✓ tick; charcoal label "Inserted"; ~400 ms then fade.
  - `error`: danger-token ⚠ glyph + short charcoal label ("Mic blocked", "No speech", "Couldn't type here");
    auto-dismiss; full detail in the Hub.
  - Pumpkin is the single accent (waveform/spinner); every state carries a **text label**, not
    color alone (≥ design-system §9c). Reduced-motion swaps animation for static indicators
    (Rule 13).
- **Tray icon**: `idle` (enabled, not dictating) · `listening` (utterance active) · `disabled`
  (dictation off — muted/struck). Tooltip shows the active model + enabled state.
- **Tray menu** (native): `Open Settings / Hub` · `Dictation enabled` (checkable) · `Model ▸`
  (submenu, active checkmarked) · separator · `Quit`.
- **Hub** (light, shown on demand): the home of all real controls/stats — see
  [settings.md](settings.md); the HUD has **no** controls (Scope decision).

---

## 7. Edge Cases

| Scenario | Expected behavior |
|---|---|
| HUD would steal focus on show | **Forbidden** — created no-activate; if a path could activate it, that's a bug (Rule 1). Injection depends on this (ADR-005). |
| Caret position unknown / not exposed by target app | Fall back to the configured screen anchor (default bottom-center of the active monitor) — Rule 11. |
| Multi-monitor, mixed DPI | HUD shows on the monitor under the foreground window, in logical px (same physical size everywhere) — Rule 14. |
| True exclusive-fullscreen app (e.g. a game) | OS may suppress the overlay; MIA doesn't fight it. **Dictation still injects**; HUD feedback may be hidden (Rule 15). |
| Foreground window is elevated (UAC) | HUD shows fine, but injection may fail (UIPI, ADR-005) → surface a danger-token ⚠ "Couldn't type here (elevated app)" and detail in the Hub. |
| Dictation disabled, hotkey pressed | No HUD, no capture; tray icon stays `disabled` (Rule 6). |
| State event dropped / dictation crashes mid-utterance | Watchdog auto-hides the HUD, tray → idle (Rule 12); HUD never orphaned. |
| "Pick model" → model not downloaded | `Err("model not downloaded")`; open the Hub download gate ([speech-to-text.md](speech-to-text.md)) — Rule 7. |
| Hub window closed | Hidden to tray, process keeps running (Rule 8); only Quit exits (Rule 9). |
| Second instance launched | Single-instance: focus existing Hub / no-op; no duplicate tray or HUD (Rule 10). |
| `prefers-reduced-motion` set | Static listening dot + non-spinning indicator; labels still change (Rule 13). |
| WebView2 compositing quirk on the pill | The pill is already a solid white surface (bg-surface) with a 2px charcoal outline — no translucency to lose; still no-activate, still positioned. |
| No tray / notification area unavailable | Log + (rare) fall back to showing the Hub; MIA must remain controllable. |

---

## 8. Testing Checklist

- **Rust** (`cargo test`, no I/O — pure helpers only):
  - [ ] `hud_anchor` — near-caret vs each screen anchor; clamps a pill fully onto a work area;
        picks the monitor under the foreground window; mixed-DPI scale handling.
  - [ ] `tray_icon_for` — idle / listening / disabled → correct icon bytes.
  - [ ] `tray_menu_model` — builds the submenu from the registry with the active model checkmarked;
        not-downloaded entries flagged.
  - [ ] `level_to_bars` — 0.0 / 1.0 / mid level → expected bar heights; clamps out-of-range input.
  - [ ] `set_active_model` / `open_hub` `Err(String)` paths return the documented messages.
- **Manual / runtime** (needs a real desktop, mic, model, focused app):
  - [ ] **Focus invariant**: dictate into Notepad/Chrome/VS Code — the target keeps focus and the
        caret throughout; text lands correctly (the core ADR-005 check, pt-BR and English).
  - [ ] HUD reflects every state: listening (waveform reacts to voice) → transcribing → inserting
        check → fade; and the error states (mic blocked, no speech, elevated app).
  - [ ] Tray icon changes idle ↔ listening ↔ disabled; tooltip shows the active model.
  - [ ] Tray menu: open Hub, toggle dictation off (PTT becomes a no-op, no HUD), pick model (swap),
        Quit (graceful — hotkey unregistered, clipboard restored, model unloaded).
  - [ ] Position: caret-anchored where supported; screen-anchored fallback; correct on the second
        monitor and at 150%/200% scaling; fullscreen app behavior.
  - [ ] Click-through: clicking "on" the pill activates the app underneath, not the HUD.
  - [ ] Reduced-motion: static indicators replace the animation; states still distinguishable.
  - [ ] Watchdog: kill/cancel mid-utterance → HUD auto-hides, tray returns to idle.
  - [ ] Second-instance launch focuses the existing app; no duplicate tray/HUD.

---

## 9. Out of Scope (this version)

- **Interactive HUD controls** (click-to-cancel, click-to-retry, editing in the pill) — clicking
  risks activation/focus-steal (Rule 1); all controls stay in the tray/Hub. Revisit only with a
  guaranteed no-activate interaction model.
- **Live partial transcript in the HUD** (showing words as they're recognized) — depends on
  streaming partials, deferred to [../ROADMAP.md](../ROADMAP.md) Phase 5 / Backlog.
- **Rich tray flyouts / mini-dashboards** beyond the native menu — stats and settings live in the
  Hub ([settings.md](settings.md)); the tray stays a minimal menu.
- **Cross-platform tray/overlay** (macOS menu-bar item / Linux tray + Wayland overlay restrictions)
  — Windows-only in V1 (ADR-011); deferred to Phase 5 / Backlog.
- **"Hey MIA" wake-word HUD affordance** and **always-listening ambient mode** — backlog
  ([../ROADMAP.md](../ROADMAP.md) Phase 5).

---

### Cross-references

- [design-system.md](design-system.md) — the Blush tokens, the white `MicHud` pill, layout §8b, UX §9.
- [dictation.md](dictation.md) — the orchestrator state machine this feature mirrors.
- [audio-capture.md](audio-capture.md) — the derived mic level meter that feeds the waveform.
- [hotkeys.md](hotkeys.md) — the PTT trigger; "dictation enabled" gates it.
- [speech-to-text.md](speech-to-text.md) — model registry / download gate behind "pick model".
- [text-injection.md](text-injection.md) — why focus-steal is fatal (ADR-005).
- [settings.md](settings.md) — the Hub; where all real controls, options, and error history live.
- [architecture.md](architecture.md) — ADR-001/002/005/006; [_template.md](_template.md).
