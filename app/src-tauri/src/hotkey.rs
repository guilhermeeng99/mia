//! Global push-to-talk hotkey (ADR-001/011, Phase 1) — the *trigger* at the front
//! of the pipeline (**hotkey** → capture → VAD → STT → cleanup → inject). It turns
//! raw key edges from the `tauri-plugin-global-shortcut` plugin into a clean,
//! debounced stream of `DictationIntent`s for the orchestrator. See
//! `docs/specs/hotkeys.md`.
//!
//! This file owns the **pure, cargo-tested core**: the accelerator parser /
//! canonicalizer, the debounce + activation-mode reducer, and the bare-key /
//! reserved-chord guards. They use small internal types (`Mods`, `Accel`) so the
//! logic is testable without the `tauri-plugin-global-shortcut` plugin or a desktop;
//! the runtime shortcut registration, the plugin's key-edge handler, and the Tauri
//! commands convert `Accel` to the plugin's `Shortcut` (`Modifiers` + `Code`) at the
//! boundary.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};
#[cfg(windows)]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_INSERT, VK_LCONTROL,
    VK_LEFT, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_NEXT, VK_PRIOR, VK_RCONTROL, VK_RETURN, VK_RIGHT,
    VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SPACE, VK_TAB, VK_UP,
};

/// Default PTT chord — `Ctrl + Space`. A modifier-anchored chord with a real key
/// (a modifier-only chord like `Ctrl+Win` is not registrable via `RegisterHotKey`),
/// low collision risk, ergonomic to hold (§4; aligned with settings.md).
pub const DEFAULT_ACCEL: &str = "Ctrl+Space";
/// Window that collapses key chatter / OS auto-repeat into one intent (Rule 9, §4).
pub const DEBOUNCE_MS: u64 = 40;
/// Missing-release watchdog timeout for push-to-hold (Rule 11, §4).
pub const MAX_HOLD_MS: u64 = 30_000;
/// Recorder auto-cancel timeout (Rule 12, §4).
pub const CAPTURE_TIMEOUT_MS: u64 = 15_000;
/// Idle self-heal cadence (Rule 15, §10): how often, while no session is active, MIA
/// re-claims the PTT chord. Windows can silently drop a `RegisterHotKey` routing (an
/// IME/TSF arming `Ctrl+Space`, a sleep/lock transition, another app briefly grabbing
/// the chord); MIA registers once at startup with no recovery, so without this the
/// hotkey stays dead until the app is restarted. Re-registering self-heals it.
pub const SELF_HEAL_INTERVAL_MS: u64 = 30_000;
/// Poll cadence of the self-heal loop — short so a cycle skipped because a hold was in
/// progress retries soon after the user lets go (Rule 15).
const SELF_HEAL_POLL_MS: u64 = 5_000;
/// Windows-key polling cadence. Needed for Win/Super and modifier-only chords that
/// `RegisterHotKey`/the Tauri plugin cannot reliably deliver.
#[cfg(windows)]
const WINDOWS_KEY_POLL_MS: u64 = 30;

/// How the chord activates dictation (§2). Persisted in settings.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum ActivationMode {
    /// Dictate while held; finish on release.
    PushToHold,
    /// Press to start, press again to stop (no need to keep holding).
    PressToToggle,
}

/// Persisted chord + mode (§2; see `settings.md`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct HotkeyConfig {
    pub accelerator: String,
    pub mode: ActivationMode,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct HotkeyRecordSample {
    pub accelerator: Option<String>,
    pub released: bool,
    pub cancelled: bool,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            accelerator: DEFAULT_ACCEL.to_string(),
            mode: ActivationMode::PushToHold,
        }
    }
}

/// The clean intents emitted to the orchestrator over `"dictation://intent"` (§2).
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum DictationIntent {
    Start,
    Stop,
    Cancel,
}

/// Modifier bitmask — our own, decoupled from `tauri-plugin-global-shortcut`'s
/// version-specific `Modifiers`, so the parser/canonicalizer stay pure and
/// unit-tested. `sup` is the Super/Win/Meta key.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub sup: bool,
}

impl Mods {
    /// True when no modifier is set — a "bare key" chord, rejected in v1 (Rule 5).
    pub fn is_empty(&self) -> bool {
        !(self.ctrl || self.alt || self.shift || self.sup)
    }
}

/// A parsed accelerator: modifiers + an optional main key (the default `Ctrl+Super`
/// is modifier-only, so `key` is `None` there).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Accel {
    pub mods: Mods,
    pub key: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Pure helpers (cargo-tested)
// ─────────────────────────────────────────────────────────────────────────────

/// True when a chord carries no modifier (Rule 5).
pub fn is_bare_key(mods: &Mods) -> bool {
    mods.is_empty()
}

/// Normalize one key token (already lowercased) to its canonical display form, or
/// `None` if it isn't a key MIA supports binding in v1.
fn canon_key(token: &str) -> Option<String> {
    if token.len() == 1 {
        let c = token.chars().next().unwrap();
        if c.is_ascii_alphabetic() {
            return Some(c.to_ascii_uppercase().to_string());
        }
        if c.is_ascii_digit() {
            return Some(c.to_string());
        }
    }
    let named = match token {
        "space" => "Space",
        "tab" => "Tab",
        "enter" | "return" => "Enter",
        "esc" | "escape" => "Escape",
        "del" | "delete" => "Delete",
        "up" => "Up",
        "down" => "Down",
        "left" => "Left",
        "right" => "Right",
        _ => return canon_function_key(token),
    };
    Some(named.to_string())
}

/// `f1`..`f24` → `F1`..`F24`, else `None`.
fn canon_function_key(token: &str) -> Option<String> {
    let rest = token.strip_prefix('f')?;
    let n: u8 = rest.parse().ok()?;
    if (1..=24).contains(&n) {
        Some(format!("F{n}"))
    } else {
        None
    }
}

/// Parse a canonical chord string into `Accel`, with the exact spec error messages
/// (§2). Accepts modifier-only chords (the default `Ctrl+Super`); rejects empty,
/// unparseable, bare-key, and `Fn` inputs.
pub fn parse_accelerator(input: &str) -> Result<Accel, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("empty hotkey".to_string());
    }
    let mut mods = Mods::default();
    let mut key: Option<String> = None;
    for part in trimmed.split('+') {
        let token = part.trim().to_lowercase();
        if token.is_empty() {
            continue;
        }
        if token == "fn" {
            return Err("Fn key is not hookable".to_string());
        }
        if apply_modifier(&token, &mut mods) {
            continue;
        }
        // A non-modifier token is the main key — only one is allowed.
        let canonical = canon_key(&token).ok_or_else(|| format!("unparseable hotkey: {input}"))?;
        if key.is_some() {
            return Err(format!("unparseable hotkey: {input}"));
        }
        key = Some(canonical);
    }
    if is_bare_key(&mods) {
        return Err("hotkey must include a modifier".to_string());
    }
    Ok(Accel { mods, key })
}

/// Set the matching modifier bit; returns `true` if `token` was a modifier.
fn apply_modifier(token: &str, mods: &mut Mods) -> bool {
    match token {
        "ctrl" | "control" => mods.ctrl = true,
        "alt" | "option" => mods.alt = true,
        "shift" => mods.shift = true,
        "super" | "win" | "windows" | "meta" | "cmd" | "command" => mods.sup = true,
        _ => return false,
    }
    true
}

/// Render an `Accel` back to its canonical string (fixed modifier order:
/// Ctrl, Alt, Shift, Super) — round-trips with `parse_accelerator` (§2).
pub fn to_canonical(accel: &Accel) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if accel.mods.ctrl {
        parts.push("Ctrl");
    }
    if accel.mods.alt {
        parts.push("Alt");
    }
    if accel.mods.shift {
        parts.push("Shift");
    }
    if accel.mods.sup {
        parts.push("Super");
    }
    if let Some(k) = &accel.key {
        parts.push(k.as_str());
    }
    parts.join("+")
}

/// True for OS-owned chords MIA must not claim (Rule 7). Checked before any OS probe.
pub fn is_reserved(accel: &Accel) -> bool {
    let key = accel.key.as_deref();
    let m = accel.mods;
    matches!(
        (m.ctrl, m.alt, m.shift, m.sup, key),
        (false, false, false, true, Some("L"))      // Win+L (lock)
            | (false, true, false, false, Some("Tab"))   // Alt+Tab
            | (false, false, false, true, Some("Tab"))   // Win+Tab
            | (false, false, false, true, Some("D"))     // Win+D (show desktop)
            | (true, true, false, false, Some("Delete")) // Ctrl+Alt+Del
    )
}

/// Pure self-heal tick decision (Rule 15): re-claim the chord only when **idle** (never
/// yank it out from under an active push-to-hold) and at most once per interval.
pub fn should_self_heal(session_active: bool, ms_since_last: u64, interval_ms: u64) -> bool {
    !session_active && ms_since_last >= interval_ms
}

// ─────────────────────────────────────────────────────────────────────────────
// Debounce + activation-mode reducer (the testable heart, §2)
// ─────────────────────────────────────────────────────────────────────────────

/// A raw chord edge from the OS layer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RawEdge {
    Pressed,
    Released,
}

/// The reducer's edge-tracking state. `active` is this module's view of whether
/// dictation is on (re-entry guard, Rule 10); `held` tracks the physical chord for
/// push-to-hold; `last_edge_ms` timestamps the last accepted edge for debounce.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct EdgeState {
    pub active: bool,
    pub held: bool,
    pub last_edge_ms: Option<u64>,
}

/// The pure debounce + mode reducer (Rules 3, 4, 9, 10). `now_ms` is a monotonic
/// millisecond clock (the runtime passes `Instant`-derived millis; tests pass plain
/// numbers). Returns the next state and any intent to emit.
pub fn reduce(
    state: EdgeState,
    edge: RawEdge,
    mode: ActivationMode,
    now_ms: u64,
    debounce_ms: u64,
) -> (EdgeState, Option<DictationIntent>) {
    if let Some(last) = state.last_edge_ms {
        if now_ms.saturating_sub(last) < debounce_ms {
            return (state, None); // chatter / auto-repeat collapsed (Rule 9)
        }
    }
    match mode {
        ActivationMode::PushToHold => reduce_push_to_hold(state, edge, now_ms),
        ActivationMode::PressToToggle => reduce_press_to_toggle(state, edge, now_ms),
    }
}

fn reduce_push_to_hold(
    state: EdgeState,
    edge: RawEdge,
    now_ms: u64,
) -> (EdgeState, Option<DictationIntent>) {
    match edge {
        RawEdge::Pressed if !state.active => (
            EdgeState {
                active: true,
                held: true,
                last_edge_ms: Some(now_ms),
            },
            Some(DictationIntent::Start),
        ),
        // Auto-repeat down while already active: ignored, no second Start (Rule 3/10).
        RawEdge::Pressed => (
            EdgeState {
                held: true,
                ..state
            },
            None,
        ),
        RawEdge::Released if state.active => (
            EdgeState {
                active: false,
                held: false,
                last_edge_ms: Some(now_ms),
            },
            Some(DictationIntent::Stop),
        ),
        // Release while inactive: nothing (Rule 10).
        RawEdge::Released => (
            EdgeState {
                held: false,
                ..state
            },
            None,
        ),
    }
}

fn reduce_press_to_toggle(
    state: EdgeState,
    edge: RawEdge,
    now_ms: u64,
) -> (EdgeState, Option<DictationIntent>) {
    match edge {
        // Toggle on the press edge only; release is irrelevant in toggle mode (Rule 4).
        RawEdge::Pressed if state.active => (
            EdgeState {
                active: false,
                last_edge_ms: Some(now_ms),
                ..state
            },
            Some(DictationIntent::Stop),
        ),
        RawEdge::Pressed => (
            EdgeState {
                active: true,
                last_edge_ms: Some(now_ms),
                ..state
            },
            Some(DictationIntent::Start),
        ),
        RawEdge::Released => (state, None),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Conversion to the tauri-plugin-global-shortcut Shortcut (pure; the runtime
// registration in lib.rs consumes this). Tested for the mapping; OS registration is
// validated on Windows.
// ─────────────────────────────────────────────────────────────────────────────

/// Map a canonical key token (`"A"`, `"Space"`, `"F8"`, `"Up"`, …) to the plugin's
/// `Code`, reusing keyboard_types' `FromStr` over the W3C code names.
pub fn key_to_code(key: &str) -> Option<Code> {
    let variant = if key.len() == 1 && key.starts_with(|c: char| c.is_ascii_alphabetic()) {
        format!("Key{}", key.to_ascii_uppercase())
    } else if key.len() == 1 && key.starts_with(|c: char| c.is_ascii_digit()) {
        format!("Digit{key}")
    } else {
        match key {
            "Up" => "ArrowUp".to_string(),
            "Down" => "ArrowDown".to_string(),
            "Left" => "ArrowLeft".to_string(),
            "Right" => "ArrowRight".to_string(),
            other => other.to_string(), // Space / Tab / Enter / Escape / Delete / F1..F24
        }
    };
    variant.parse::<Code>().ok()
}

/// Convert a parsed `Accel` into a registrable `Shortcut` (needs a non-modifier key;
/// a modifier-only chord is not registrable — Rule 5 / Ctrl+Space default).
pub fn to_shortcut(accel: &Accel) -> Result<Shortcut, String> {
    let mut mods = Modifiers::empty();
    if accel.mods.ctrl {
        mods |= Modifiers::CONTROL;
    }
    if accel.mods.alt {
        mods |= Modifiers::ALT;
    }
    if accel.mods.shift {
        mods |= Modifiers::SHIFT;
    }
    if accel.mods.sup {
        mods |= Modifiers::SUPER;
    }
    let key = accel.key.as_deref().ok_or("hotkey must include a key")?;
    let code = key_to_code(key).ok_or_else(|| format!("unparseable hotkey: {key}"))?;
    Ok(Shortcut::new(Some(mods), code))
}

#[cfg(windows)]
fn should_poll_accel(accel: &Accel) -> bool {
    accel.mods.sup || accel.key.is_none()
}

#[cfg(not(windows))]
fn should_poll_accel(_accel: &Accel) -> bool {
    false
}

fn plugin_shortcut(accel: &Accel) -> Result<Option<Shortcut>, String> {
    if should_poll_accel(accel) {
        Ok(None)
    } else {
        to_shortcut(accel).map(Some)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Runtime registration + event loop (tauri-plugin-global-shortcut; validated on Windows)
// ─────────────────────────────────────────────────────────────────────────────

/// Managed runtime state: the active config + the reducer's edge tracker.
pub struct HotkeyRuntime {
    cfg: Mutex<HotkeyConfig>,
    edge: Mutex<EdgeState>,
    suspended: AtomicBool,
    /// Monotonic session generation: each Start bumps it. A watchdog armed for an
    /// older generation no-ops, so a real Stop/Cancel silently disarms it (Rule 11).
    generation: AtomicU64,
}

impl HotkeyRuntime {
    pub fn new(cfg: HotkeyConfig) -> Self {
        Self {
            cfg: Mutex::new(cfg),
            edge: Mutex::new(EdgeState::default()),
            suspended: AtomicBool::new(false),
            generation: AtomicU64::new(0),
        }
    }

    /// Replace the active config with the saved one at startup. Managed (default
    /// chord) on the builder so `get_hotkey` is race-proof against an early
    /// frontend invoke; `setup` hydrates it once the saved config is loaded.
    pub fn hydrate(&self, cfg: HotkeyConfig) {
        if let Ok(mut guard) = self.cfg.lock() {
            *guard = cfg;
        }
    }
}

/// The transient `Escape` binding registered only while a session is active (Rule 8).
fn escape_shortcut() -> Shortcut {
    Shortcut::new(None, Code::Escape)
}

fn is_escape(shortcut: &Shortcut) -> bool {
    shortcut == &escape_shortcut()
}

/// Route a raw global-shortcut edge from the plugin handler (`lib.rs`). `Escape` —
/// registered only while a session is active — cancels; every other chord is the PTT
/// binding and drives the reducer. (The plugin handler can't tell them apart, so we
/// dispatch on the fired `Shortcut` here.)
pub fn on_shortcut(app: &AppHandle, shortcut: &Shortcut, pressed: bool) {
    if is_escape(shortcut) {
        if pressed {
            cancel_from_escape(app);
        }
        return;
    }
    on_shortcut_event(app, pressed);
}

/// Drive the pure `reduce` from a raw PTT-chord edge and emit the resulting intent to
/// the frontend over `dictation://intent`; Start/Stop also manage the session's
/// transient Esc binding + missing-release watchdog.
pub fn on_shortcut_event(app: &AppHandle, pressed: bool) {
    let Some(rt) = app.try_state::<HotkeyRuntime>() else {
        return;
    };
    if rt.suspended.load(Ordering::SeqCst) {
        return;
    }
    let Ok(mode) = rt.cfg.lock().map(|c| c.mode) else {
        return;
    };
    let edge = if pressed {
        RawEdge::Pressed
    } else {
        RawEdge::Released
    };
    let intent = {
        let Ok(mut e) = rt.edge.lock() else { return };
        let (next, intent) = reduce(*e, edge, mode, crate::persist::now_ms(), DEBOUNCE_MS);
        *e = next;
        intent
    };
    let Some(intent) = intent else {
        return;
    };
    crate::dlog!(
        "[hotkey] {} → intent {intent:?}",
        if pressed { "down" } else { "up" }
    );
    // Emit FIRST — the orchestrator drives off this. Then run the session side-effects
    // (Esc binding + watchdog) on a SEPARATE thread: we are *inside* the plugin's
    // shortcut handler, so calling register/unregister here would re-enter the plugin
    // lock and deadlock (freezing the app). A worker thread blocks harmlessly until the
    // handler returns and releases the lock.
    let _ = app.emit("dictation://intent", intent);
    let app = app.clone();
    std::thread::spawn(move || match intent {
        DictationIntent::Start => on_session_start(&app),
        DictationIntent::Stop | DictationIntent::Cancel => on_session_end(&app),
    });
}

/// Begin a session: register the transient Esc-cancel binding and (push-to-hold only)
/// arm the missing-release watchdog. MUST run off the shortcut-handler thread.
fn on_session_start(app: &AppHandle) {
    let _ = app.global_shortcut().register(escape_shortcut());
    let push_to_hold = app
        .try_state::<HotkeyRuntime>()
        .and_then(|rt| {
            rt.cfg
                .lock()
                .ok()
                .map(|c| c.mode == ActivationMode::PushToHold)
        })
        .unwrap_or(false);
    if push_to_hold {
        arm_watchdog(app);
    }
}

/// End a session: bump the generation (disarms any pending watchdog) and drop the
/// transient Esc binding so Escape is free again everywhere else.
fn on_session_end(app: &AppHandle) {
    if let Some(rt) = app.try_state::<HotkeyRuntime>() {
        rt.generation.fetch_add(1, Ordering::SeqCst);
    }
    let _ = app.global_shortcut().unregister(escape_shortcut());
}

/// Esc pressed during a session → reset the reducer and emit Cancel (Rule 8). Emits
/// first, then defers the unregister off the handler thread (re-entrancy deadlock).
fn cancel_from_escape(app: &AppHandle) {
    crate::dlog!("[hotkey] Esc → intent Cancel");
    let _ = app.emit("dictation://intent", DictationIntent::Cancel);
    let app = app.clone();
    std::thread::spawn(move || {
        if let Some(rt) = app.try_state::<HotkeyRuntime>() {
            rt.generation.fetch_add(1, Ordering::SeqCst);
            if let Ok(mut e) = rt.edge.lock() {
                *e = EdgeState::default();
            }
        }
        let _ = app.global_shortcut().unregister(escape_shortcut());
    });
}

/// Missing-release watchdog (Rule 11): if a push-to-hold session never sees its
/// release within `MAX_HOLD_MS` (e.g. a focus change ate the keyup), force a Stop so
/// dictation can't get stuck "listening" forever. A real Stop/Cancel (or a fresh
/// session) bumps the generation first, so this fires only for a genuinely stuck hold.
fn arm_watchdog(app: &AppHandle) {
    let Some(rt) = app.try_state::<HotkeyRuntime>() else {
        return;
    };
    let generation = rt.generation.load(Ordering::SeqCst);
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(MAX_HOLD_MS));
        let Some(rt) = app.try_state::<HotkeyRuntime>() else {
            return;
        };
        if rt.generation.load(Ordering::SeqCst) != generation {
            return; // a real edge already ended this session
        }
        let active = rt.edge.lock().map(|e| e.active).unwrap_or(false);
        if !active {
            return;
        }
        if let Ok(mut e) = rt.edge.lock() {
            *e = EdgeState::default();
        }
        rt.generation.fetch_add(1, Ordering::SeqCst);
        let _ = app.global_shortcut().unregister(escape_shortcut());
        crate::dlog!("[hotkey] watchdog: no release after {MAX_HOLD_MS}ms → forcing Stop");
        let _ = app.emit("dictation://intent", DictationIntent::Stop);
    });
}

/// Register `cfg`'s chord, replacing any prior registration (Rule 1; rolls back the
/// stored config only after the OS accepts the new chord).
pub fn register_hotkey_runtime(
    app: &AppHandle,
    rt: &HotkeyRuntime,
    cfg: HotkeyConfig,
) -> Result<(), String> {
    let accel = parse_accelerator(&cfg.accelerator)?;
    if is_reserved(&accel) {
        return Err(format!(
            "hotkey already in use by the system or another app: {}",
            cfg.accelerator
        ));
    }
    let shortcut = plugin_shortcut(&accel)?;
    let gs = app.global_shortcut();
    let prev = rt
        .cfg
        .lock()
        .map_err(|_| "hotkey state poisoned".to_string())?
        .clone();
    let prev_shortcut = parse_accelerator(&prev.accelerator)
        .ok()
        .and_then(|a| plugin_shortcut(&a).ok().flatten());
    let same_shortcut = shortcut
        .as_ref()
        .is_some_and(|sc| prev_shortcut.as_ref().is_some_and(|prev_sc| prev_sc == sc));
    if !same_shortcut {
        if let Some(sc) = shortcut {
            gs.register(sc)
                .map_err(|e| format!("hotkey already in use by the system or another app: {e}"))?;
        }
        if let Some(prev_sc) = prev_shortcut {
            let _ = gs.unregister(prev_sc);
        }
    }
    *rt.cfg
        .lock()
        .map_err(|_| "hotkey state poisoned".to_string())? = cfg;
    *rt.edge
        .lock()
        .map_err(|_| "hotkey state poisoned".to_string())? = EdgeState::default();
    rt.suspended.store(false, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub fn register_hotkey(
    app: AppHandle,
    rt: State<'_, HotkeyRuntime>,
    cfg: HotkeyConfig,
) -> Result<(), String> {
    register_hotkey_runtime(&app, &rt, cfg)
}

/// Unregister the active chord (idempotent).
#[tauri::command]
pub fn unregister_hotkey(app: AppHandle, rt: State<'_, HotkeyRuntime>) -> Result<(), String> {
    let prev = rt
        .cfg
        .lock()
        .map_err(|_| "hotkey state poisoned".to_string())?
        .clone();
    if let Some(sc) = parse_accelerator(&prev.accelerator)
        .ok()
        .and_then(|a| plugin_shortcut(&a).ok().flatten())
    {
        let _ = app.global_shortcut().unregister(sc);
    }
    rt.suspended.store(true, Ordering::SeqCst);
    *rt.edge
        .lock()
        .map_err(|_| "hotkey state poisoned".to_string())? = EdgeState::default();
    Ok(())
}

/// = unregister + register (persists only on OS acceptance).
#[tauri::command]
pub fn update_hotkey(
    app: AppHandle,
    rt: State<'_, HotkeyRuntime>,
    cfg: HotkeyConfig,
) -> Result<(), String> {
    register_hotkey_runtime(&app, &rt, cfg)
}

/// The active chord + mode.
#[tauri::command]
pub fn get_hotkey(rt: State<'_, HotkeyRuntime>) -> Result<HotkeyConfig, String> {
    Ok(rt
        .cfg
        .lock()
        .map_err(|_| "hotkey state poisoned".to_string())?
        .clone())
}

#[tauri::command]
pub fn sample_hotkey_recording() -> HotkeyRecordSample {
    platform_record_sample()
}

/// Best-effort startup registration from the saved config (Rule 14: a conflict
/// leaves the chord unregistered rather than stealing a different key).
pub fn register_initial(app: &AppHandle, cfg: &HotkeyConfig) {
    match parse_accelerator(&cfg.accelerator).and_then(|a| plugin_shortcut(&a)) {
        Ok(Some(sc)) => match app.global_shortcut().register(sc) {
            Ok(()) => crate::dlog!("[hotkey] PTT '{}' registered", cfg.accelerator),
            // The likeliest cause on Windows is the chord already being claimed (an
            // IME often owns Ctrl+Space) — surface it instead of failing silently.
            Err(e) => eprintln!(
                "[hotkey] PTT '{}' FAILED to register (likely claimed by the OS/another app): {e}",
                cfg.accelerator
            ),
        },
        Ok(None) => crate::dlog!(
            "[hotkey] PTT '{}' registered via Windows polling",
            cfg.accelerator
        ),
        Err(e) => eprintln!("[hotkey] PTT '{}' is invalid: {e}", cfg.accelerator),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Self-healing re-registration (Rule 15) — the recovery the single startup
// registration above lacks. A silently-dropped OS routing is otherwise permanent
// until restart; these re-claim the chord automatically (idle tick + resume/unlock)
// and on demand (tray "Re-register").
// ─────────────────────────────────────────────────────────────────────────────

/// Re-claim the active PTT chord on the OS (Rule 15). Idempotent: unregister-then-
/// register, so a still-valid binding is a harmless no-op and a silently-dropped one is
/// restored. Resets the edge tracker — a fresh OS registration invalidates any in-flight
/// hold. **MUST run off the plugin's shortcut-handler thread AND off the main loop**:
/// `register`/`unregister` post to the main loop and block, so calling them there would
/// deadlock (see `on_shortcut_event`). Callers reach it via `request_reregister` (its own
/// worker) or the `start_self_heal` tick (its own thread).
fn do_reregister(app: &AppHandle) {
    let Some(rt) = app.try_state::<HotkeyRuntime>() else {
        return;
    };
    let Ok(cfg) = rt.cfg.lock().map(|c| c.clone()) else {
        return;
    };
    let Ok(accel) = parse_accelerator(&cfg.accelerator) else {
        return;
    };
    if should_poll_accel(&accel) {
        if let Ok(mut e) = rt.edge.lock() {
            *e = EdgeState::default();
        }
        crate::dlog!(
            "[hotkey] self-heal: PTT '{}' is handled by Windows polling",
            cfg.accelerator
        );
        return;
    }
    let Ok(sc) = to_shortcut(&accel) else {
        return;
    };
    let gs = app.global_shortcut();
    let _ = gs.unregister(sc); // ignore: may not currently be registered (Copy, no move)
    match gs.register(sc) {
        Ok(()) => {
            if let Ok(mut e) = rt.edge.lock() {
                *e = EdgeState::default();
            }
            crate::dlog!(
                "[hotkey] self-heal: re-registered PTT '{}'",
                cfg.accelerator
            );
        }
        // Chord momentarily owned by the OS/another app (e.g. an IME armed Ctrl+Space):
        // leave it; the next tick retries and re-claims it once they release it.
        Err(e) => {
            crate::dlog!(
                "[hotkey] self-heal: re-register deferred ('{}'): {e}",
                cfg.accelerator
            )
        }
    }
}

/// Request a PTT re-registration from anywhere (tray menu, window focus, resume/unlock).
/// Spawns a worker so it is safe to call from the main loop / event callbacks — calling
/// the plugin's register/unregister directly there would deadlock (see `do_reregister`).
pub fn request_reregister(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || do_reregister(&app));
}

/// Start the idle self-heal loop (Rule 15): every `SELF_HEAL_INTERVAL_MS`, while no
/// session is active, re-claim the PTT chord so a silently-dropped registration recovers
/// without a restart. Spawned once from `setup`; runs for the app's lifetime.
pub fn start_self_heal(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let mut last = crate::persist::now_ms();
        loop {
            std::thread::sleep(Duration::from_millis(SELF_HEAL_POLL_MS));
            let active = app
                .try_state::<HotkeyRuntime>()
                .and_then(|rt| rt.edge.lock().ok().map(|e| e.active))
                .unwrap_or(false);
            if should_self_heal(
                active,
                crate::persist::now_ms().saturating_sub(last),
                SELF_HEAL_INTERVAL_MS,
            ) {
                do_reregister(&app);
                last = crate::persist::now_ms();
            }
        }
    });
}

#[cfg(windows)]
pub fn start_windows_key_polling(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let mut last_accelerator = String::new();
        let mut active = false;
        let mut blocked_until_release = false;
        loop {
            std::thread::sleep(Duration::from_millis(WINDOWS_KEY_POLL_MS));
            let Some(rt) = app.try_state::<HotkeyRuntime>() else {
                continue;
            };
            if rt.suspended.load(Ordering::SeqCst) {
                active = false;
                blocked_until_release = false;
                continue;
            }
            let Ok(cfg) = rt.cfg.lock().map(|c| c.clone()) else {
                continue;
            };
            let Ok(accel) = parse_accelerator(&cfg.accelerator) else {
                active = false;
                blocked_until_release = false;
                continue;
            };
            if !should_poll_accel(&accel) {
                active = false;
                blocked_until_release = false;
                continue;
            }
            if cfg.accelerator != last_accelerator {
                last_accelerator = cfg.accelerator.clone();
                active = false;
                blocked_until_release = false;
            }
            let pressed = polled_accel_pressed(&accel, &mut blocked_until_release);
            if pressed != active {
                active = pressed;
                on_shortcut_event(&app, pressed);
            }
        }
    });
}

#[cfg(not(windows))]
pub fn start_windows_key_polling(_app: &AppHandle) {}

#[cfg(windows)]
fn polled_accel_pressed(accel: &Accel, blocked_until_release: &mut bool) -> bool {
    if !expected_modifiers_pressed(accel.mods) {
        *blocked_until_release = false;
        return false;
    }
    if *blocked_until_release {
        return false;
    }
    if accel.key.is_none() && any_non_modifier_pressed() {
        *blocked_until_release = true;
        return false;
    }
    accel.key.as_deref().is_none_or(key_pressed)
}

#[cfg(windows)]
fn expected_modifiers_pressed(expected: Mods) -> bool {
    let ctrl = any_pressed(&[VK_LCONTROL as i32, VK_RCONTROL as i32]);
    let alt = any_pressed(&[VK_LMENU as i32, VK_RMENU as i32]);
    let shift = any_pressed(&[VK_LSHIFT as i32, VK_RSHIFT as i32]);
    let sup = any_pressed(&[VK_LWIN as i32, VK_RWIN as i32]);

    expected_modifiers_match(
        expected,
        Mods {
            ctrl,
            alt,
            shift,
            sup,
        },
    )
}

fn expected_modifiers_match(expected: Mods, actual: Mods) -> bool {
    (!expected.ctrl || actual.ctrl)
        && (!expected.alt || actual.alt)
        && (!expected.shift || actual.shift)
        && (!expected.sup || actual.sup)
}

#[cfg(windows)]
fn key_pressed(key: &str) -> bool {
    if key.len() == 1 {
        let c = key.chars().next().unwrap();
        if c.is_ascii_alphabetic() {
            return vk_pressed(c.to_ascii_uppercase() as i32);
        }
        if c.is_ascii_digit() {
            return vk_pressed(c as i32);
        }
    }
    match key {
        "Space" => vk_pressed(VK_SPACE as i32),
        "Tab" => vk_pressed(VK_TAB as i32),
        "Enter" => vk_pressed(VK_RETURN as i32),
        "Escape" => vk_pressed(VK_ESCAPE as i32),
        "Delete" => vk_pressed(VK_DELETE as i32),
        "Up" => vk_pressed(VK_UP as i32),
        "Down" => vk_pressed(VK_DOWN as i32),
        "Left" => vk_pressed(VK_LEFT as i32),
        "Right" => vk_pressed(VK_RIGHT as i32),
        key if key.starts_with('F') => key[1..]
            .parse::<i32>()
            .ok()
            .filter(|n| (1..=24).contains(n))
            .is_some_and(|n| vk_pressed(0x6F + n)),
        _ => false,
    }
}

#[cfg(windows)]
fn platform_record_sample() -> HotkeyRecordSample {
    let mods = current_mods_pressed();
    let key = first_pressed_record_key();
    let released = mods.is_empty() && key.is_none();
    let cancelled = mods.is_empty() && key.as_deref() == Some("Escape");
    let accelerator = if cancelled {
        None
    } else if let Some(key) = key {
        (!mods.is_empty()).then(|| to_canonical(&Accel {
            mods,
            key: Some(key),
        }))
    } else {
        (modifier_count(mods) >= 2).then(|| to_canonical(&Accel { mods, key: None }))
    };

    HotkeyRecordSample {
        accelerator,
        released,
        cancelled,
    }
}

#[cfg(not(windows))]
fn platform_record_sample() -> HotkeyRecordSample {
    HotkeyRecordSample {
        accelerator: None,
        released: true,
        cancelled: false,
    }
}

#[cfg(windows)]
fn current_mods_pressed() -> Mods {
    Mods {
        ctrl: any_pressed(&[VK_LCONTROL as i32, VK_RCONTROL as i32]),
        alt: any_pressed(&[VK_LMENU as i32, VK_RMENU as i32]),
        shift: any_pressed(&[VK_LSHIFT as i32, VK_RSHIFT as i32]),
        sup: any_pressed(&[VK_LWIN as i32, VK_RWIN as i32]),
    }
}

fn modifier_count(mods: Mods) -> usize {
    [mods.ctrl, mods.alt, mods.shift, mods.sup]
        .into_iter()
        .filter(|pressed| *pressed)
        .count()
}

#[cfg(windows)]
fn first_pressed_record_key() -> Option<String> {
    let named = [
        ("Space", VK_SPACE as i32),
        ("Tab", VK_TAB as i32),
        ("Enter", VK_RETURN as i32),
        ("Escape", VK_ESCAPE as i32),
        ("Delete", VK_DELETE as i32),
        ("Up", VK_UP as i32),
        ("Down", VK_DOWN as i32),
        ("Left", VK_LEFT as i32),
        ("Right", VK_RIGHT as i32),
    ];
    for (key, vk) in named {
        if vk_pressed(vk) {
            return Some(key.to_string());
        }
    }
    for vk in 0x41i32..=0x5A {
        if vk_pressed(vk) {
            return char::from_u32(vk as u32).map(|c| c.to_string());
        }
    }
    for vk in 0x30i32..=0x39 {
        if vk_pressed(vk) {
            return char::from_u32(vk as u32).map(|c| c.to_string());
        }
    }
    for n in 1..=24 {
        if vk_pressed(0x6F + n) {
            return Some(format!("F{n}"));
        }
    }
    None
}

#[cfg(windows)]
fn any_pressed(keys: &[i32]) -> bool {
    keys.iter().copied().any(vk_pressed)
}

#[cfg(windows)]
fn vk_pressed(vk: i32) -> bool {
    unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 }
}

#[cfg(windows)]
fn any_non_modifier_pressed() -> bool {
    const VK_BACK: i32 = 0x08;
    let nav = [
        VK_BACK,
        VK_TAB as i32,
        VK_RETURN as i32,
        VK_SPACE as i32,
        VK_PRIOR as i32,
        VK_NEXT as i32,
        VK_END as i32,
        VK_HOME as i32,
        VK_LEFT as i32,
        VK_UP as i32,
        VK_RIGHT as i32,
        VK_DOWN as i32,
        VK_INSERT as i32,
        VK_DELETE as i32,
    ];
    if nav.iter().copied().any(vk_pressed) {
        return true;
    }
    (0x30i32..=0x39)
        .chain(0x41..=0x5A)
        .chain(0x70..=0x87)
        .any(vk_pressed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accel(s: &str) -> Accel {
        parse_accelerator(s).unwrap()
    }

    #[test]
    fn parses_canonical_chords() {
        assert_eq!(
            accel("Ctrl+Super"),
            Accel {
                mods: Mods {
                    ctrl: true,
                    sup: true,
                    ..Default::default()
                },
                key: None
            }
        );
        assert_eq!(
            accel("Alt+Space"),
            Accel {
                mods: Mods {
                    alt: true,
                    ..Default::default()
                },
                key: Some("Space".into())
            }
        );
        assert_eq!(
            accel("Ctrl+Shift+D"),
            Accel {
                mods: Mods {
                    ctrl: true,
                    shift: true,
                    ..Default::default()
                },
                key: Some("D".into())
            }
        );
    }

    #[test]
    fn parse_is_case_and_alias_insensitive() {
        assert_eq!(accel("control+win"), accel("Ctrl+Super"));
        assert_eq!(accel("ALT+space"), accel("Alt+Space"));
    }

    #[test]
    fn rejects_empty_unparseable_bare_and_fn() {
        assert_eq!(parse_accelerator("   "), Err("empty hotkey".into()));
        assert_eq!(
            parse_accelerator("Ctrl+Nope"),
            Err("unparseable hotkey: Ctrl+Nope".into())
        );
        assert_eq!(
            parse_accelerator("D"),
            Err("hotkey must include a modifier".into())
        );
        assert_eq!(
            parse_accelerator("F8"),
            Err("hotkey must include a modifier".into())
        );
        assert_eq!(
            parse_accelerator("Fn"),
            Err("Fn key is not hookable".into())
        );
        assert_eq!(
            parse_accelerator("Ctrl+Fn"),
            Err("Fn key is not hookable".into())
        );
    }

    #[test]
    fn rejects_two_main_keys() {
        assert!(parse_accelerator("Ctrl+A+B").is_err());
    }

    #[test]
    fn to_canonical_round_trips() {
        for s in ["Ctrl+Super", "Alt+Space", "Ctrl+Shift+D", "Ctrl+Alt+F4"] {
            let a = accel(s);
            assert_eq!(parse_accelerator(&to_canonical(&a)).unwrap(), a);
        }
    }

    #[test]
    fn reserved_chords_flagged() {
        assert!(is_reserved(&accel("Super+L")));
        assert!(is_reserved(&accel("Alt+Tab")));
        assert!(is_reserved(&accel("Ctrl+Alt+Delete")));
        assert!(is_reserved(&accel("Super+D")));
        assert!(!is_reserved(&accel("Ctrl+Super")));
        assert!(!is_reserved(&accel("Ctrl+Shift+D")));
    }

    #[cfg(windows)]
    #[test]
    fn windows_key_chords_use_polling() {
        assert!(should_poll_accel(&accel("Ctrl+Win")));
        assert!(should_poll_accel(&accel("Win+Space")));
        assert!(!should_poll_accel(&accel("Ctrl+Space")));
        assert!(plugin_shortcut(&accel("Ctrl+Win")).unwrap().is_none());
        assert!(plugin_shortcut(&accel("Ctrl+Space")).unwrap().is_some());
    }

    #[cfg(windows)]
    #[test]
    fn polled_modifier_match_allows_extra_modifiers() {
        let expected = Mods {
            sup: true,
            ..Default::default()
        };
        assert!(expected_modifiers_match(
            expected,
            Mods {
                ctrl: true,
                sup: true,
                ..Default::default()
            }
        ));
        assert!(!expected_modifiers_match(
            expected,
            Mods {
                ctrl: true,
                ..Default::default()
            }
        ));
    }

    #[test]
    fn push_to_hold_down_starts_repeat_ignored_release_stops() {
        let s0 = EdgeState::default();
        let (s1, i1) = reduce(
            s0,
            RawEdge::Pressed,
            ActivationMode::PushToHold,
            100,
            DEBOUNCE_MS,
        );
        assert_eq!(i1, Some(DictationIntent::Start));
        assert!(s1.active);
        // Auto-repeat down beyond the debounce window: no second Start (Rule 3/10).
        let (s2, i2) = reduce(
            s1,
            RawEdge::Pressed,
            ActivationMode::PushToHold,
            1_000,
            DEBOUNCE_MS,
        );
        assert_eq!(i2, None);
        assert!(s2.active);
        let (s3, i3) = reduce(
            s2,
            RawEdge::Released,
            ActivationMode::PushToHold,
            2_000,
            DEBOUNCE_MS,
        );
        assert_eq!(i3, Some(DictationIntent::Stop));
        assert!(!s3.active);
        // Release while inactive: nothing (Rule 10).
        let (_s4, i4) = reduce(
            s3,
            RawEdge::Released,
            ActivationMode::PushToHold,
            3_000,
            DEBOUNCE_MS,
        );
        assert_eq!(i4, None);
    }

    #[test]
    fn press_to_toggle_alternates_and_ignores_release() {
        let mut t = 0;
        let fire = |s: EdgeState, edge: RawEdge, t: u64| {
            reduce(s, edge, ActivationMode::PressToToggle, t, DEBOUNCE_MS)
        };
        let (s1, i1) = fire(EdgeState::default(), RawEdge::Pressed, {
            t += 100;
            t
        });
        assert_eq!(i1, Some(DictationIntent::Start));
        let (s2, i2) = fire(s1, RawEdge::Released, {
            t += 100;
            t
        });
        assert_eq!(i2, None); // release ignored in toggle mode
        let (s3, i3) = fire(s2, RawEdge::Pressed, {
            t += 100;
            t
        });
        assert_eq!(i3, Some(DictationIntent::Stop));
        let (s4, i4) = fire(s3, RawEdge::Pressed, {
            t += 100;
            t
        });
        assert_eq!(i4, Some(DictationIntent::Start));
        assert!(s4.active);
    }

    #[test]
    fn debounce_collapses_edges_within_window() {
        let s0 = EdgeState::default();
        let (s1, i1) = reduce(
            s0,
            RawEdge::Pressed,
            ActivationMode::PressToToggle,
            100,
            DEBOUNCE_MS,
        );
        assert_eq!(i1, Some(DictationIntent::Start));
        // Second press 20 ms later (< 40 ms debounce) is dropped.
        let (s2, i2) = reduce(
            s1,
            RawEdge::Pressed,
            ActivationMode::PressToToggle,
            120,
            DEBOUNCE_MS,
        );
        assert_eq!(i2, None);
        assert!(s2.active); // unchanged
    }

    #[test]
    fn re_entry_guard_no_stop_while_inactive() {
        let s0 = EdgeState::default();
        let (_s, i) = reduce(
            s0,
            RawEdge::Released,
            ActivationMode::PushToHold,
            100,
            DEBOUNCE_MS,
        );
        assert_eq!(i, None);
    }

    #[test]
    fn key_to_code_maps_known_keys() {
        assert!(key_to_code("Space").is_some());
        assert!(key_to_code("A").is_some());
        assert!(key_to_code("F8").is_some());
        assert!(key_to_code("Up").is_some());
        assert!(key_to_code("1").is_some());
        assert!(key_to_code("Nope").is_none());
    }

    #[test]
    fn to_shortcut_needs_a_key() {
        assert!(to_shortcut(&accel("Ctrl+Space")).is_ok());
        assert!(to_shortcut(&accel("Ctrl+Shift+D")).is_ok());
        // A modifier-only chord has no key → not registrable.
        let mods_only = Accel {
            mods: Mods {
                ctrl: true,
                sup: true,
                ..Default::default()
            },
            key: None,
        };
        assert!(to_shortcut(&mods_only).is_err());
    }

    #[test]
    fn self_heal_only_when_idle_and_interval_elapsed() {
        // Idle and a full interval since the last re-claim → re-register (Rule 15).
        assert!(should_self_heal(false, 30_000, 30_000));
        assert!(should_self_heal(false, 45_000, 30_000));
        // Active hold → never (don't yank the chord out from under an in-flight session).
        assert!(!should_self_heal(true, 60_000, 30_000));
        // Idle but the interval hasn't elapsed yet → wait.
        assert!(!should_self_heal(false, 10_000, 30_000));
    }

    #[test]
    fn default_config_is_ctrl_space_push_to_hold() {
        let c = HotkeyConfig::default();
        assert_eq!(c.accelerator, "Ctrl+Space");
        assert_eq!(c.mode, ActivationMode::PushToHold);
        let parsed = parse_accelerator(&c.accelerator).unwrap();
        assert_eq!(parsed.key.as_deref(), Some("Space")); // a registrable chord (has a key)
    }
}
