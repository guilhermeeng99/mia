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

use tauri::{AppHandle, Manager, Monitor, PhysicalPosition, WebviewWindow};

const HUD_LABEL: &str = "hud";
/// Gap (physical px) between the pill and the screen's bottom edge.
#[cfg(not(target_os = "macos"))]
const BOTTOM_MARGIN: i32 = 64;
/// macOS: gap (points) above the Dock / bottom of the usable area. The work area
/// already excludes the Dock, so this is just breathing room, scaled per display.
#[cfg(target_os = "macos")]
const MAC_DOCK_GAP_PT: f64 = 16.0;

/// Make the HUD click-through and dock it bottom-center of its monitor. Best-effort:
/// a missing window or monitor must never fail app startup, so every step is lenient.
pub fn setup_hud(app: &AppHandle) {
    let Some(hud) = app.get_webview_window(HUD_LABEL) else {
        return;
    };
    // Mouse events fall through to the app beneath — the HUD never needs input.
    let _ = hud.set_ignore_cursor_events(true);
    // macOS pins a window to the Space it opened on; without this the pill stays
    // behind on Desktop 1 while the user dictates in another Space.
    #[cfg(target_os = "macos")]
    let _ = hud.set_visible_on_all_workspaces(true);
    dock_bottom_center(&hud);
}

/// Re-assert the HUD's topmost z-order. WHY: Windows can silently demote an
/// always-on-top window out of the HWND_TOPMOST band (fullscreen/exclusive apps,
/// other topmost windows, an Explorer restart), after which the pill still runs
/// but renders *behind* the foreground app. `set_always_on_top(true)` issues a
/// fresh SetWindowPos(HWND_TOPMOST) without activating the window (so focus never
/// moves — ADR-005), and it is cheap, so the engine calls this on every phase
/// change (dictation::show_phase).
pub fn assert_topmost(app: &AppHandle) {
    let Some(hud) = app.get_webview_window(HUD_LABEL) else {
        return;
    };
    let _ = hud.set_always_on_top(true);
    // macOS: the Dock can be resized, magnified, moved or auto-hidden at any time, so
    // re-dock above its current edge at the start of every phase too.
    #[cfg(target_os = "macos")]
    dock_bottom_center(&hud);
}

/// Centered bottom-edge placement, clamped to the monitor's origin so the window never
/// lands off the top/left of the screen. Pure arithmetic, unit-tested in isolation —
/// `origin`/`screen`/`win` are physical px, `margin` is the gap above the bottom edge.
fn bottom_center_pos(origin: (i32, i32), screen: (i32, i32), win: (i32, i32), margin: i32) -> (i32, i32) {
    let (ox, oy) = origin;
    let (sw, sh) = screen;
    let (ww, wh) = win;
    let x = ox + (sw - ww) / 2;
    let y = oy + sh - wh - margin;
    (x.max(ox), y.max(oy))
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
    let (origin, screen, margin) = placement_area(&monitor);
    let (x, y) = bottom_center_pos(origin, screen, (win.width as i32, win.height as i32), margin);
    let _ = hud.set_position(PhysicalPosition::new(x, y));
}

/// Windows: the full monitor with a fixed margin that clears the taskbar (validated
/// behavior, kept as is).
#[cfg(not(target_os = "macos"))]
fn placement_area(monitor: &Monitor) -> ((i32, i32), (i32, i32), i32) {
    let (origin, size) = (monitor.position(), monitor.size());
    ((origin.x, origin.y), (size.width as i32, size.height as i32), BOTTOM_MARGIN)
}

/// macOS: the monitor's work area (NSScreen `visibleFrame` — minus the Dock and the
/// menu bar), so the pill sits above the Dock whatever its size, side or auto-hide
/// setting, instead of being painted behind it.
#[cfg(target_os = "macos")]
fn placement_area(monitor: &Monitor) -> ((i32, i32), (i32, i32), i32) {
    let area = monitor.work_area();
    let margin = (MAC_DOCK_GAP_PT * monitor.scale_factor()).round() as i32;
    (
        (area.position.x, area.position.y),
        (area.size.width as i32, area.size.height as i32),
        margin,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centers_and_offsets_above_the_bottom_edge() {
        // 1920x1080 monitor at origin, 200x48 pill, 64px margin.
        let (x, y) = bottom_center_pos((0, 0), (1920, 1080), (200, 48), 64);
        assert_eq!(x, (1920 - 200) / 2); // 860
        assert_eq!(y, 1080 - 48 - 64); // 968
    }

    #[test]
    fn honors_a_nonzero_monitor_origin() {
        // A second monitor to the right keeps the pill on that screen.
        let (x, y) = bottom_center_pos((1920, 0), (1280, 720), (200, 48), 64);
        assert_eq!(x, 1920 + (1280 - 200) / 2);
        assert_eq!(y, 720 - 48 - 64);
    }

    /// tauri.macos.conf.json must restate both windows (a JSON merge patch replaces the
    /// whole `windows` array), so guard it against drifting from the base config: only
    /// the native-titlebar keys of the Hub may differ.
    #[test]
    fn macos_window_config_only_adds_the_native_titlebar() {
        let parse = |s: &str| serde_json::from_str::<serde_json::Value>(s).unwrap();
        let base = parse(include_str!("../tauri.conf.json"));
        let mut mac = parse(include_str!("../tauri.macos.conf.json"));
        let main = mac["app"]["windows"][0].as_object_mut().unwrap();
        for key in ["titleBarStyle", "hiddenTitle", "trafficLightPosition"] {
            assert!(main.remove(key).is_some(), "{key} missing from the macOS Hub window");
        }
        main.insert("decorations".into(), false.into());
        main.insert("shadow".into(), false.into());
        assert_eq!(mac["app"]["windows"], base["app"]["windows"]);
    }

    #[test]
    fn clamps_to_origin_when_window_exceeds_screen() {
        // A window taller/wider than the monitor must never land above/left of origin.
        let (x, y) = bottom_center_pos((100, 100), (300, 200), (400, 400), 64);
        assert_eq!(x, 100);
        assert_eq!(y, 100);
    }
}
