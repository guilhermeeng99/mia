//! System tray (Tauri 2 built-in `tray-icon`) — MIA lives in the tray (ADR / Phase
//! 1). The Hub window is opened from here; closing the window hides to tray rather
//! than quitting. See `docs/specs/tray-and-hud.md`. Runtime-validated on Windows.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

/// Build the tray icon + menu (Open / Quit). Called once from `setup`.
pub fn init(app: &AppHandle) -> Result<(), String> {
    let open = MenuItem::with_id(app, "open", "Abrir MIA", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let quit = MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let menu = Menu::with_items(app, &[&open, &quit]).map_err(|e| e.to_string())?;
    let icon = app.default_window_icon().ok_or("no app icon")?.clone();

    TrayIconBuilder::with_id("mia-tray")
        .icon(icon)
        .tooltip("MIA — ditado local")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "quit" => app.exit(0),
            "open" => show_main(app),
            _ => {}
        })
        .build(app)
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Show + focus the main Hub window (from the tray "Open" action).
fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}
