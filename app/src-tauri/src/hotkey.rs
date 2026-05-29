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

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};

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

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self { accelerator: DEFAULT_ACCEL.to_string(), mode: ActivationMode::PushToHold }
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
            EdgeState { active: true, held: true, last_edge_ms: Some(now_ms) },
            Some(DictationIntent::Start),
        ),
        // Auto-repeat down while already active: ignored, no second Start (Rule 3/10).
        RawEdge::Pressed => (EdgeState { held: true, ..state }, None),
        RawEdge::Released if state.active => (
            EdgeState { active: false, held: false, last_edge_ms: Some(now_ms) },
            Some(DictationIntent::Stop),
        ),
        // Release while inactive: nothing (Rule 10).
        RawEdge::Released => (EdgeState { held: false, ..state }, None),
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
            EdgeState { active: false, last_edge_ms: Some(now_ms), ..state },
            Some(DictationIntent::Stop),
        ),
        RawEdge::Pressed => (
            EdgeState { active: true, last_edge_ms: Some(now_ms), ..state },
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

// ─────────────────────────────────────────────────────────────────────────────
// Runtime registration + event loop (tauri-plugin-global-shortcut; validated on Windows)
// ─────────────────────────────────────────────────────────────────────────────

/// Managed runtime state: the active config + the reducer's edge tracker.
pub struct HotkeyRuntime {
    cfg: Mutex<HotkeyConfig>,
    edge: Mutex<EdgeState>,
    /// Monotonic session generation: each Start bumps it. A watchdog armed for an
    /// older generation no-ops, so a real Stop/Cancel silently disarms it (Rule 11).
    generation: AtomicU64,
}

impl HotkeyRuntime {
    pub fn new(cfg: HotkeyConfig) -> Self {
        Self {
            cfg: Mutex::new(cfg),
            edge: Mutex::new(EdgeState::default()),
            generation: AtomicU64::new(0),
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

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
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
    let Ok(mode) = rt.cfg.lock().map(|c| c.mode) else {
        return;
    };
    let edge = if pressed { RawEdge::Pressed } else { RawEdge::Released };
    let intent = {
        let Ok(mut e) = rt.edge.lock() else { return };
        let (next, intent) = reduce(*e, edge, mode, now_ms(), DEBOUNCE_MS);
        *e = next;
        intent
    };
    let Some(intent) = intent else {
        return;
    };
    eprintln!("[hotkey] {} → intent {intent:?}", if pressed { "down" } else { "up" });
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
        .and_then(|rt| rt.cfg.lock().ok().map(|c| c.mode == ActivationMode::PushToHold))
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
    eprintln!("[hotkey] Esc → intent Cancel");
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
        eprintln!("[hotkey] watchdog: no release after {MAX_HOLD_MS}ms → forcing Stop");
        let _ = app.emit("dictation://intent", DictationIntent::Stop);
    });
}

/// Register `cfg`'s chord, replacing any prior registration (Rule 1; rolls back the
/// stored config only after the OS accepts the new chord).
#[tauri::command]
pub fn register_hotkey(app: AppHandle, rt: State<'_, HotkeyRuntime>, cfg: HotkeyConfig) -> Result<(), String> {
    let accel = parse_accelerator(&cfg.accelerator)?;
    if is_reserved(&accel) {
        return Err(format!("hotkey already in use by the system or another app: {}", cfg.accelerator));
    }
    let shortcut = to_shortcut(&accel)?;
    let gs = app.global_shortcut();
    let prev = rt.cfg.lock().map_err(|_| "hotkey state poisoned".to_string())?.clone();
    if let Ok(prev_sc) = parse_accelerator(&prev.accelerator).and_then(|a| to_shortcut(&a)) {
        let _ = gs.unregister(prev_sc);
    }
    gs.register(shortcut)
        .map_err(|e| format!("hotkey already in use by the system or another app: {e}"))?;
    *rt.cfg.lock().map_err(|_| "hotkey state poisoned".to_string())? = cfg;
    *rt.edge.lock().map_err(|_| "hotkey state poisoned".to_string())? = EdgeState::default();
    Ok(())
}

/// Unregister the active chord (idempotent).
#[tauri::command]
pub fn unregister_hotkey(app: AppHandle, rt: State<'_, HotkeyRuntime>) -> Result<(), String> {
    let prev = rt.cfg.lock().map_err(|_| "hotkey state poisoned".to_string())?.clone();
    if let Ok(sc) = parse_accelerator(&prev.accelerator).and_then(|a| to_shortcut(&a)) {
        let _ = app.global_shortcut().unregister(sc);
    }
    Ok(())
}

/// = unregister + register (persists only on OS acceptance).
#[tauri::command]
pub fn update_hotkey(app: AppHandle, rt: State<'_, HotkeyRuntime>, cfg: HotkeyConfig) -> Result<(), String> {
    register_hotkey(app, rt, cfg)
}

/// The active chord + mode.
#[tauri::command]
pub fn get_hotkey(rt: State<'_, HotkeyRuntime>) -> Result<HotkeyConfig, String> {
    Ok(rt.cfg.lock().map_err(|_| "hotkey state poisoned".to_string())?.clone())
}

/// Best-effort startup registration from the saved config (Rule 14: a conflict
/// leaves the chord unregistered rather than stealing a different key).
pub fn register_initial(app: &AppHandle, cfg: &HotkeyConfig) {
    match parse_accelerator(&cfg.accelerator).and_then(|a| to_shortcut(&a)) {
        Ok(sc) => match app.global_shortcut().register(sc) {
            Ok(()) => eprintln!("[hotkey] PTT '{}' registered", cfg.accelerator),
            // The likeliest cause on Windows is the chord already being claimed (an
            // IME often owns Ctrl+Space) — surface it instead of failing silently.
            Err(e) => eprintln!(
                "[hotkey] PTT '{}' FAILED to register (likely claimed by the OS/another app): {e}",
                cfg.accelerator
            ),
        },
        Err(e) => eprintln!("[hotkey] PTT '{}' is invalid: {e}", cfg.accelerator),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accel(s: &str) -> Accel {
        parse_accelerator(s).unwrap()
    }

    #[test]
    fn parses_canonical_chords() {
        assert_eq!(accel("Ctrl+Super"), Accel { mods: Mods { ctrl: true, sup: true, ..Default::default() }, key: None });
        assert_eq!(accel("Alt+Space"), Accel { mods: Mods { alt: true, ..Default::default() }, key: Some("Space".into()) });
        assert_eq!(accel("Ctrl+Shift+D"), Accel { mods: Mods { ctrl: true, shift: true, ..Default::default() }, key: Some("D".into()) });
    }

    #[test]
    fn parse_is_case_and_alias_insensitive() {
        assert_eq!(accel("control+win"), accel("Ctrl+Super"));
        assert_eq!(accel("ALT+space"), accel("Alt+Space"));
    }

    #[test]
    fn rejects_empty_unparseable_bare_and_fn() {
        assert_eq!(parse_accelerator("   "), Err("empty hotkey".into()));
        assert_eq!(parse_accelerator("Ctrl+Nope"), Err("unparseable hotkey: Ctrl+Nope".into()));
        assert_eq!(parse_accelerator("D"), Err("hotkey must include a modifier".into()));
        assert_eq!(parse_accelerator("F8"), Err("hotkey must include a modifier".into()));
        assert_eq!(parse_accelerator("Fn"), Err("Fn key is not hookable".into()));
        assert_eq!(parse_accelerator("Ctrl+Fn"), Err("Fn key is not hookable".into()));
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

    #[test]
    fn push_to_hold_down_starts_repeat_ignored_release_stops() {
        let s0 = EdgeState::default();
        let (s1, i1) = reduce(s0, RawEdge::Pressed, ActivationMode::PushToHold, 100, DEBOUNCE_MS);
        assert_eq!(i1, Some(DictationIntent::Start));
        assert!(s1.active);
        // Auto-repeat down beyond the debounce window: no second Start (Rule 3/10).
        let (s2, i2) = reduce(s1, RawEdge::Pressed, ActivationMode::PushToHold, 1_000, DEBOUNCE_MS);
        assert_eq!(i2, None);
        assert!(s2.active);
        let (s3, i3) = reduce(s2, RawEdge::Released, ActivationMode::PushToHold, 2_000, DEBOUNCE_MS);
        assert_eq!(i3, Some(DictationIntent::Stop));
        assert!(!s3.active);
        // Release while inactive: nothing (Rule 10).
        let (_s4, i4) = reduce(s3, RawEdge::Released, ActivationMode::PushToHold, 3_000, DEBOUNCE_MS);
        assert_eq!(i4, None);
    }

    #[test]
    fn press_to_toggle_alternates_and_ignores_release() {
        let mut t = 0;
        let fire = |s: EdgeState, edge: RawEdge, t: u64| {
            reduce(s, edge, ActivationMode::PressToToggle, t, DEBOUNCE_MS)
        };
        let (s1, i1) = fire(EdgeState::default(), RawEdge::Pressed, { t += 100; t });
        assert_eq!(i1, Some(DictationIntent::Start));
        let (s2, i2) = fire(s1, RawEdge::Released, { t += 100; t });
        assert_eq!(i2, None); // release ignored in toggle mode
        let (s3, i3) = fire(s2, RawEdge::Pressed, { t += 100; t });
        assert_eq!(i3, Some(DictationIntent::Stop));
        let (s4, i4) = fire(s3, RawEdge::Pressed, { t += 100; t });
        assert_eq!(i4, Some(DictationIntent::Start));
        assert!(s4.active);
    }

    #[test]
    fn debounce_collapses_edges_within_window() {
        let s0 = EdgeState::default();
        let (s1, i1) = reduce(s0, RawEdge::Pressed, ActivationMode::PressToToggle, 100, DEBOUNCE_MS);
        assert_eq!(i1, Some(DictationIntent::Start));
        // Second press 20 ms later (< 40 ms debounce) is dropped.
        let (s2, i2) = reduce(s1, RawEdge::Pressed, ActivationMode::PressToToggle, 120, DEBOUNCE_MS);
        assert_eq!(i2, None);
        assert!(s2.active); // unchanged
    }

    #[test]
    fn re_entry_guard_no_stop_while_inactive() {
        let s0 = EdgeState::default();
        let (_s, i) = reduce(s0, RawEdge::Released, ActivationMode::PushToHold, 100, DEBOUNCE_MS);
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
        let mods_only = Accel { mods: Mods { ctrl: true, sup: true, ..Default::default() }, key: None };
        assert!(to_shortcut(&mods_only).is_err());
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
