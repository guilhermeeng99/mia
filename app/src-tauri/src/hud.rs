//! The floating mic HUD overlay window (tray-and-hud.md §, design-system.md §8b).
//!
//! The HUD is a **separate** always-on-top, transparent, click-through window so the
//! "listening → transcribing → inserting" feedback floats over whatever app the user
//! is dictating into — the main Hub window is usually hidden or behind it. Two hard
//! constraints drive the design:
//!
//! 1. **It must never take focus**, or text injection (SendInput) would target the
//!    HUD instead of the user's app (ADR-005). Hence `focus: false` in `tauri.conf`
//!    and we never call `show`/`set_focus` on it.
//! 2. **Clicks must pass through** to the app underneath — the HUD is pure feedback,
//!    never interactive — so we make the whole window click-through here.
//!
//! The webview (`HudWindow.svelte`) renders the pill, driven by the engine's
//! `hud://state` events. This module only does the native plumbing the frontend
//! can't: click-through + docking bottom-center.

use tauri::{AppHandle, Manager, PhysicalPosition, WebviewWindow};

const HUD_LABEL: &str = "hud";
/// Gap (physical px) between the pill and the screen's bottom edge.
const BOTTOM_MARGIN: i32 = 64;

/// Make the HUD click-through and dock it bottom-center of its monitor. Best-effort:
/// a missing window or monitor must never fail app startup, so every step is lenient.
pub fn setup_hud(app: &AppHandle) {
    let Some(hud) = app.get_webview_window(HUD_LABEL) else {
        return;
    };
    // Mouse events fall through to the app beneath — the HUD never needs input.
    let _ = hud.set_ignore_cursor_events(true);
    dock_bottom_center(&hud);
}

/// Position the (physically sized) window centered along its monitor's bottom edge,
/// accounting for the monitor's own origin so it lands on the right screen.
fn dock_bottom_center(hud: &WebviewWindow) {
    let Ok(Some(monitor)) = hud.current_monitor() else {
        return;
    };
    let Ok(win) = hud.outer_size() else {
        return;
    };
    let screen = monitor.size();
    let origin = monitor.position();
    let x = origin.x + (screen.width as i32 - win.width as i32) / 2;
    let y = origin.y + screen.height as i32 - win.height as i32 - BOTTOM_MARGIN;
    let _ = hud.set_position(PhysicalPosition::new(x.max(origin.x), y.max(origin.y)));
}
