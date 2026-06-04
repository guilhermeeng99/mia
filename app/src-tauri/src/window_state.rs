//! Main-window geometry persistence.
//!
//! Settings are user preferences; this file is runtime UI state. Keeping it in a
//! separate store lets bounds be updated opportunistically from window events
//! without rewriting the primary settings tree during drags/resizes.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Manager, Monitor, PhysicalPosition, PhysicalSize, Runtime, WebviewWindow, Window,
};

const MAIN_WINDOW_LABEL: &str = "main";
const STORE_FILE: &str = "window-state.json";
const SCHEMA_VERSION: u32 = 1;
const MIN_WIDTH: u32 = 720;
const MIN_HEIGHT: u32 = 520;
const MAX_DIMENSION: u32 = 16_000;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct WindowStateStore {
    #[serde(default = "default_schema_version")]
    schema_version: u32,
    #[serde(default)]
    main: Option<MainWindowState>,
}

impl Default for WindowStateStore {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            main: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct MainWindowState {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    #[serde(default)]
    maximized: bool,
}

impl MainWindowState {
    fn from_bounds(
        position: PhysicalPosition<i32>,
        size: PhysicalSize<u32>,
        maximized: bool,
    ) -> Self {
        Self {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
            maximized,
        }
    }

    fn has_valid_bounds(self) -> bool {
        self.width >= MIN_WIDTH
            && self.height >= MIN_HEIGHT
            && self.width <= MAX_DIMENSION
            && self.height <= MAX_DIMENSION
    }
}

fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

pub fn restore_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(saved) = load_main_window_state(app) else {
        return;
    };
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return;
    };

    let size = PhysicalSize::new(saved.width, saved.height);
    let position = clamped_position_for_window(&window, saved)
        .unwrap_or(PhysicalPosition::new(saved.x, saved.y));

    let _ = window.set_size(size);
    let _ = window.set_position(position);
    if saved.maximized {
        let _ = window.maximize();
    }
}

pub fn save_main_window_bounds<R: Runtime>(window: &Window<R>) -> Result<(), String> {
    if window.label() != MAIN_WINDOW_LABEL {
        return Ok(());
    }
    if window.is_minimized().unwrap_or(false) {
        return Ok(());
    }

    let app = window.app_handle();
    let maximized = window.is_maximized().unwrap_or(false);
    let position = window.outer_position().map_err(|e| e.to_string())?;
    let size = window.outer_size().map_err(|e| e.to_string())?;
    save_bounds(app, position, size, maximized)
}

pub fn save_main_webview_window_bounds<R: Runtime>(
    window: &WebviewWindow<R>,
) -> Result<(), String> {
    if window.label() != MAIN_WINDOW_LABEL {
        return Ok(());
    }
    if window.is_minimized().unwrap_or(false) {
        return Ok(());
    }

    let app = window.app_handle();
    let maximized = window.is_maximized().unwrap_or(false);
    let position = window.outer_position().map_err(|e| e.to_string())?;
    let size = window.outer_size().map_err(|e| e.to_string())?;
    save_bounds(app, position, size, maximized)
}

fn save_bounds<R: Runtime>(
    app: &AppHandle<R>,
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    maximized: bool,
) -> Result<(), String> {
    let mut next = MainWindowState::from_bounds(position, size, maximized);
    if maximized {
        if let Some(previous) = load_main_window_state(app) {
            next.x = previous.x;
            next.y = previous.y;
            next.width = previous.width;
            next.height = previous.height;
        }
    }
    if !next.has_valid_bounds() {
        return Ok(());
    }

    let mut store = load_store(app);
    store.schema_version = SCHEMA_VERSION;
    store.main = Some(next);
    save_store(app, &store)
}

fn load_main_window_state<R: Runtime>(app: &AppHandle<R>) -> Option<MainWindowState> {
    load_store(app)
        .main
        .filter(|state| state.has_valid_bounds())
}

fn state_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join(STORE_FILE))
}

fn load_store<R: Runtime>(app: &AppHandle<R>) -> WindowStateStore {
    let Ok(path) = state_path(app) else {
        return WindowStateStore::default();
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_store<R: Runtime>(app: &AppHandle<R>, store: &WindowStateStore) -> Result<(), String> {
    let path = state_path(app)?;
    crate::persist::atomic_write_json(&path, store)
}

fn clamped_position_for_window<R: Runtime>(
    window: &WebviewWindow<R>,
    state: MainWindowState,
) -> Option<PhysicalPosition<i32>> {
    let monitors = window.available_monitors().ok()?;
    let monitor = monitors
        .iter()
        .find(|monitor| bounds_intersect_monitor(state, monitor))
        .or_else(|| monitors.first())?;
    let origin = monitor.position();
    let screen = monitor.size();
    let (x, y) = clamp_position_to_screen(
        (state.x, state.y),
        (origin.x, origin.y),
        (screen.width as i32, screen.height as i32),
        (state.width as i32, state.height as i32),
    );
    Some(PhysicalPosition::new(x, y))
}

fn bounds_intersect_monitor(state: MainWindowState, monitor: &Monitor) -> bool {
    let origin = monitor.position();
    let size = monitor.size();
    rects_intersect(
        (
            state.x as i64,
            state.y as i64,
            state.width as i64,
            state.height as i64,
        ),
        (
            origin.x as i64,
            origin.y as i64,
            size.width as i64,
            size.height as i64,
        ),
    )
}

fn rects_intersect(a: (i64, i64, i64, i64), b: (i64, i64, i64, i64)) -> bool {
    let (ax, ay, aw, ah) = a;
    let (bx, by, bw, bh) = b;
    ax < bx + bw && ax + aw > bx && ay < by + bh && ay + ah > by
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_sensible_bounds_only() {
        assert!(MainWindowState {
            x: 100,
            y: 120,
            width: 1070,
            height: 850,
            maximized: false,
        }
        .has_valid_bounds());
        assert!(!MainWindowState {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
            maximized: false,
        }
        .has_valid_bounds());
    }

    #[test]
    fn detects_intersecting_rects() {
        assert!(rects_intersect((100, 100, 800, 600), (0, 0, 1920, 1080)));
        assert!(!rects_intersect((2500, 100, 800, 600), (0, 0, 1920, 1080)));
    }

    #[test]
    fn clamps_to_visible_screen_area() {
        let pos = clamp_position_to_screen((2600, 120), (0, 0), (1920, 1080), (920, 680));
        assert_eq!(pos, (1000, 120));
    }
}
