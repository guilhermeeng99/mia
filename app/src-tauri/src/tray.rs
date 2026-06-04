//! System tray (Tauri 2 built-in `tray-icon`) — MIA lives in the tray (ADR / Phase
//! 1). The Hub window is opened from here; closing the window hides to tray rather
//! than quitting. See `docs/specs/tray-and-hud.md`. Runtime-validated on Windows.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewWindow};

/// Build the tray icon + menu (Open / Quit). Called once from `setup`.
pub fn init(app: &AppHandle) -> Result<(), String> {
    let open = MenuItem::with_id(app, "open", "Abrir MIA", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    // Instant manual recovery for the global hotkey (hotkeys.md Rule 15): if Ctrl+Space
    // stops firing because its OS registration was silently dropped, this re-claims it
    // without the close-and-reopen workaround.
    let reregister = MenuItem::with_id(app, "reregister", "Reativar atalho", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let quit =
        MenuItem::with_id(app, "quit", "Sair", true, None::<&str>).map_err(|e| e.to_string())?;
    let menu = Menu::with_items(app, &[&open, &reregister, &quit]).map_err(|e| e.to_string())?;
    let icon = app.default_window_icon().ok_or("no app icon")?.clone();

    TrayIconBuilder::with_id("mia-tray")
        .icon(icon)
        .tooltip("MIA — ditado local")
        .menu(&menu)
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_main(tray.app_handle());
            }
        })
        .on_menu_event(|app, event| match event.id.as_ref() {
            "quit" => quit_app(app),
            "open" => show_main(app),
            "reregister" => crate::hotkey::request_reregister(app),
            _ => {}
        })
        .build(app)
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Show + focus the main Hub window (from the tray "Open" action).
fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        keep_on_screen(&window);
        let _ = window.set_focus();
    }
}

fn keep_on_screen(window: &WebviewWindow) {
    let Ok(size) = window.outer_size() else {
        return;
    };
    let Ok(position) = window.outer_position() else {
        return;
    };
    let monitor = match window.current_monitor() {
        Ok(Some(monitor)) => Some(monitor),
        _ => window.primary_monitor().ok().flatten(),
    };
    let Some(monitor) = monitor else {
        return;
    };

    let origin = monitor.position();
    let screen = monitor.size();
    let (x, y) = clamp_position_to_screen(
        (position.x, position.y),
        (origin.x, origin.y),
        (screen.width as i32, screen.height as i32),
        (size.width as i32, size.height as i32),
    );

    if x != position.x || y != position.y {
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
}

fn clamp_position_to_screen(
    position: (i32, i32),
    origin: (i32, i32),
    screen: (i32, i32),
    window: (i32, i32),
) -> (i32, i32) {
    let (px, py) = position;
    let (ox, oy) = origin;
    let (sw, sh) = screen;
    let (ww, wh) = window;

    let max_x = ox + (sw - ww).max(0);
    let max_y = oy + (sh - wh).max(0);
    (px.clamp(ox, max_x), py.clamp(oy, max_y))
}

fn quit_app(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = crate::window_state::save_main_webview_window_bounds(&window);
    }
    if let Some(stt) = app.try_state::<crate::stt::SttState>() {
        let _ = crate::stt::unload(&stt);
    }
    app.exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_a_visible_window_in_place() {
        let pos = clamp_position_to_screen((100, 120), (0, 0), (1920, 1080), (920, 680));
        assert_eq!(pos, (100, 120));
    }

    #[test]
    fn brings_a_window_back_from_outside_the_right_edge() {
        let pos = clamp_position_to_screen((2600, 120), (0, 0), (1920, 1080), (920, 680));
        assert_eq!(pos, (1000, 120));
    }

    #[test]
    fn honors_monitors_with_nonzero_origin() {
        let pos = clamp_position_to_screen((-500, -300), (-1920, 0), (1920, 1080), (920, 680));
        assert_eq!(pos, (-920, 0));
    }

    #[test]
    fn pins_oversized_windows_to_the_monitor_origin() {
        let pos = clamp_position_to_screen((500, 500), (100, 100), (300, 200), (920, 680));
        assert_eq!(pos, (100, 100));
    }
}
