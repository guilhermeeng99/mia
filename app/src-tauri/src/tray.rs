//! System tray (Tauri 2 built-in `tray-icon`) — MIA lives in the tray (ADR / Phase
//! 1). The Hub window is opened from here; closing the window hides to tray rather
//! than quitting. The tray icon can also serve as a **dictation recording indicator**:
//! a colored corner badge (`reflect_phase`) shows the live phase in the notification
//! area even when the Hub is hidden — one of the user-selectable indicator options
//! (overlay / tray / both, see settings `hud.indicator`), alongside the floating HUD.
//! See `docs/specs/tray-and-hud.md`. Runtime-validated on Windows.

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewWindow};

use crate::dictation::Phase;

/// Stable id so the engine can fetch the tray (`app.tray_by_id`) to re-skin it per phase.
const TRAY_ID: &str = "mia-tray";

/// Build the tray icon + menu (Open / Quit). Called once from `setup`.
pub fn init(app: &AppHandle) -> Result<(), String> {
    let open = MenuItem::with_id(app, "open", "Abrir MIA", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let quit =
        MenuItem::with_id(app, "quit", "Sair", true, None::<&str>).map_err(|e| e.to_string())?;
    let menu = Menu::with_items(app, &[&open, &quit]).map_err(|e| e.to_string())?;
    let icon = app.default_window_icon().ok_or("no app icon")?.clone();

    TrayIconBuilder::with_id(TRAY_ID)
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
            _ => {}
        })
        .build(app)
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Recording indicator (tray-icon badge — one of the indicator options)
// ─────────────────────────────────────────────────────────────────────────────

/// Recording dot — `#E53E3E`. Listening = an utterance is actively being captured.
const REC_RED: [u8; 3] = [0xE5, 0x3E, 0x3E];
/// Busy dot — `#F2A033` (pumpkin). Transcribing / inserting (model + injection).
const BUSY_AMBER: [u8; 3] = [0xF2, 0xA0, 0x33];

/// A colored dot painted over the brand icon, with an optional soft outer glow.
/// Position, core radius, and glow radius are fractions of the icon's smaller side so
/// they scale with whatever tray-icon resolution the OS requests. `glow_frac <= r_frac`
/// disables the glow (no halo).
#[derive(Clone, Copy)]
struct Dot {
    cx_frac: f32,
    cy_frac: f32,
    r_frac: f32,
    glow_frac: f32,
    rgb: [u8; 3],
}

/// Peak opacity of the glow halo at the core edge (0..1), fading to 0 at `glow_frac`.
const GLOW_CENTER: f32 = 0.55;

/// The recording dot: a **big** red ball in the top-right corner with a soft red glow
/// behind it — the unmistakable "rec" cue while listening (the reference mock, bigger).
const REC_DOT: Dot =
    Dot { cx_frac: 0.74, cy_frac: 0.26, r_frac: 0.25, glow_frac: 0.46, rgb: REC_RED };
/// The busy dot: a smaller amber ball (no glow), bottom-right, while STT/injection runs.
const BUSY_DOT: Dot =
    Dot { cx_frac: 0.76, cy_frac: 0.76, r_frac: 0.16, glow_frac: 0.0, rgb: BUSY_AMBER };

/// Reflect the live dictation phase on the tray icon + tooltip — the tray recording
/// indicator (used for the "tray"/"both" options). Called from `dictation.rs` only
/// when the user enabled it, so it updates even when the Hub window is hidden.
/// Best-effort: a missing tray must never break the pipeline, so every step is lenient.
pub fn reflect_phase(app: &AppHandle, phase: Phase, message: Option<&str>) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let _ = tray.set_tooltip(Some(phase_tooltip(phase, message)));
    if let Some(icon) = phase_icon(app, phase) {
        let _ = tray.set_icon(Some(icon));
    }
}

/// The tray icon for a phase: the brand icon with a big red dot while **listening**,
/// a smaller amber dot while transcribing/inserting, and the plain brand icon when
/// idle or after the transient error.
fn phase_icon(app: &AppHandle, phase: Phase) -> Option<Image<'static>> {
    match phase {
        Phase::Listening => badge_icon(app, REC_DOT),
        Phase::Transcribing | Phase::Inserting => badge_icon(app, BUSY_DOT),
        Phase::Idle | Phase::Error => default_icon(app),
    }
}

/// The plain brand icon as an owned (`'static`) tray `Image`. Rebuilds it from the
/// default window icon's pixels so the lifetime isn't tied to the borrow.
fn default_icon(app: &AppHandle) -> Option<Image<'static>> {
    let base = app.default_window_icon()?;
    Some(Image::new_owned(base.rgba().to_vec(), base.width(), base.height()))
}

/// The tooltip line per phase (pt-BR). `message` enriches the transient error.
fn phase_tooltip(phase: Phase, message: Option<&str>) -> String {
    match phase {
        Phase::Idle => "MIA — ditado local".to_string(),
        Phase::Listening => "MIA — ouvindo…".to_string(),
        Phase::Transcribing => "MIA — transcrevendo…".to_string(),
        Phase::Inserting => "MIA — inserindo…".to_string(),
        Phase::Error => match message {
            Some(m) => format!("MIA — erro: {m}"),
            None => "MIA — erro".to_string(),
        },
    }
}

/// Build a tray icon by painting `dot` onto a copy of the brand icon's pixels.
/// Returns `None` if there's no default icon to base it on.
fn badge_icon(app: &AppHandle, dot: Dot) -> Option<Image<'static>> {
    let base = app.default_window_icon()?;
    let (w, h) = (base.width(), base.height());
    let mut rgba = base.rgba().to_vec();
    overlay_dot(&mut rgba, w, h, dot);
    Some(Image::new_owned(rgba, w, h))
}

/// Paint a filled circular dot (plus its soft glow halo) onto an RGBA icon, in place,
/// at `dot`'s fractional center/radius. The solid core is alpha-blended with a ~1px
/// anti-aliased edge; outside it, the glow fades linearly from `GLOW_CENTER` opacity at
/// the core edge to 0 at `glow_frac`. Pure pixel math → cargo-tested.
fn overlay_dot(rgba: &mut [u8], w: u32, h: u32, dot: Dot) {
    let (wi, hi) = (w as i32, h as i32);
    let min = wi.min(hi) as f32;
    let core = min * dot.r_frac;
    let glow = min * dot.glow_frac;
    let cx = wi as f32 * dot.cx_frac;
    let cy = hi as f32 * dot.cy_frac;
    for y in 0..hi {
        for x in 0..wi {
            let (dx, dy) = (x as f32 - cx, y as f32 - cy);
            let dist = (dx * dx + dy * dy).sqrt();
            let core_cover = (core - dist).clamp(0.0, 1.0);
            let glow_alpha = if dist < glow {
                (GLOW_CENTER * (1.0 - dist / glow)).max(0.0)
            } else {
                0.0
            };
            let a = core_cover.max(glow_alpha);
            if a <= 0.0 {
                continue;
            }
            let i = ((y * wi + x) * 4) as usize;
            rgba[i] = blend(rgba[i], dot.rgb[0], a);
            rgba[i + 1] = blend(rgba[i + 1], dot.rgb[1], a);
            rgba[i + 2] = blend(rgba[i + 2], dot.rgb[2], a);
            rgba[i + 3] = blend(rgba[i + 3], 0xFF, a);
        }
    }
}

/// Alpha-blend `fg` over `bg` with coverage `a` in `[0.0, 1.0]`.
fn blend(bg: u8, fg: u8, a: f32) -> u8 {
    (fg as f32 * a + bg as f32 * (1.0 - a)).round() as u8
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

    #[test]
    fn recording_dot_is_red_and_bigger_than_busy() {
        // Guard the "rec = big red ball" intent by painting both dots: listening is red
        // at its center and covers more pixels than the amber busy dot.
        let painted = |dot: Dot| {
            let mut rgba = vec![0u8; 32 * 32 * 4];
            overlay_dot(&mut rgba, 32, 32, dot);
            rgba.chunks_exact(4).filter(|p| p[3] > 0).count()
        };
        let mut rec = vec![0u8; 32 * 32 * 4];
        overlay_dot(&mut rec, 32, 32, REC_DOT);
        let cx = (REC_DOT.cx_frac * 32.0) as usize;
        let cy = (REC_DOT.cy_frac * 32.0) as usize;
        let c = (cy * 32 + cx) * 4;
        assert_eq!(&rec[c..c + 4], &[REC_RED[0], REC_RED[1], REC_RED[2], 0xFF]);
        assert!(painted(REC_DOT) > painted(BUSY_DOT));
    }

    #[test]
    fn error_tooltip_includes_the_message_when_present() {
        assert_eq!(phase_tooltip(Phase::Listening, None), "MIA — ouvindo…");
        assert_eq!(phase_tooltip(Phase::Error, None), "MIA — erro");
        assert_eq!(
            phase_tooltip(Phase::Error, Some("microfone bloqueado")),
            "MIA — erro: microfone bloqueado"
        );
    }

    #[test]
    fn blend_is_opaque_and_transparent_at_the_extremes() {
        assert_eq!(blend(0, 0xE5, 1.0), 0xE5); // full coverage → pure foreground
        assert_eq!(blend(0x10, 0xE5, 0.0), 0x10); // zero coverage → untouched background
        assert_eq!(blend(0, 100, 0.5), 50); // half coverage → midpoint
    }

    #[test]
    fn dot_paints_its_center_and_leaves_the_far_corner_clear() {
        // 16x16 transparent icon; the busy dot centers at (~11.8, ~11.8), so a pixel
        // there is an opaque amber dot while the top-left corner stays untouched.
        let mut rgba = vec![0u8; 16 * 16 * 4];
        overlay_dot(&mut rgba, 16, 16, BUSY_DOT);

        let center = ((12 * 16 + 12) * 4) as usize; // inside the dot
        assert_eq!(&rgba[center..center + 4], &[BUSY_AMBER[0], BUSY_AMBER[1], BUSY_AMBER[2], 0xFF]);

        let tl = 0usize; // top-left — well outside the dot
        assert_eq!(&rgba[tl..tl + 4], &[0, 0, 0, 0]);
    }
}
