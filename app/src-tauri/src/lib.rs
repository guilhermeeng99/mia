//! MIA — local voice-to-text dictation for Windows.
//!
//! Rust is the engine (see `CLAUDE.md` and `docs/specs/architecture.md`). This
//! file owns the Tauri bootstrap and the `#[tauri::command]` registry; see the
//! `invoke_handler!` block in `run()` for the live command set spanning the full
//! pipeline (audio, dictation, hotkey, stt, settings, dictionary, snippets, stats,
//! inject) plus the tray and the floating HUD window.

pub mod app_styles;
pub mod audio;
pub mod cleanup;
pub mod dictation;
pub mod dictionary;
pub mod history;
pub mod hotkey;
pub mod hud;
pub mod inject;
pub mod persist;
pub mod power_resume;
pub mod settings;
pub mod snippets;
pub mod stats;
pub mod stt;
pub mod text_match;
pub mod tray;
pub mod vad;
pub mod win32;

use tauri::{Manager, WindowEvent};

/// Debug-only stderr tracing for the dictation pipeline. The shipped GUI has no
/// attached console, so these traces compile out of release builds; genuine
/// warnings (e.g. a failed hotkey registration) stay as plain `eprintln!`.
#[macro_export]
macro_rules! dlog {
    ($($arg:tt)*) => {{
        #[cfg(debug_assertions)]
        eprintln!($($arg)*);
    }};
}

/// Return the running app version (compiled in from Cargo). Trivial by design —
/// it is the scaffold's IPC smoke test, called via the `appVersion()` wrapper
/// (`app/src/lib/app.ts`).
#[tauri::command]
fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[cfg(windows)]
fn apply_main_window_rounded_corners<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use windows_sys::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    };

    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    let Ok(handle) = win.hwnd() else {
        return;
    };
    let pref = DWMWCP_ROUND;

    unsafe {
        let _ = DwmSetWindowAttribute(
            handle.0 as _,
            DWMWA_WINDOW_CORNER_PREFERENCE as _,
            &pref as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&pref) as u32,
        );
    }
}

#[cfg(not(windows))]
fn apply_main_window_rounded_corners<R: tauri::Runtime>(_app: &tauri::AppHandle<R>) {}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // The warm whisper-server lives once in managed state, shared across
        // every utterance (ADR-004) — never a cold spawn per utterance.
        .manage(stt::SttState::default())
        .manage(audio::CaptureState::default())
        // Focused-app captured at session start → per-app style + UIPI checks (dictation.rs).
        .manage(dictation::FocusContext::default())
        // Disk-backed state is managed up-front with defaults so commands are
        // race-proof: the packaged webview loads from disk faster than the dev
        // server, so the frontend can `invoke` before `setup` runs. Managing here
        // (not in `setup`) guarantees the state exists before any IPC; `setup`
        // then hydrates each from disk once the app handle is available.
        .manage(settings::SettingsState::new(settings::Settings::default()))
        .manage(hotkey::HotkeyRuntime::new(hotkey::HotkeyConfig::default()))
        .manage(stats::StatsState::new(stats::UsageStats::default()))
        .manage(dictionary::DictState::new(
            Vec::new(),
            dictionary::DictSettings::default(),
        ))
        .manage(history::HistoryState::new(Vec::new()))
        .manage(snippets::SnippetState::new(Vec::new()))
        // Global push-to-talk: the plugin delivers key edges; the handler runs the
        // pure reducer and emits `dictation://intent` for the frontend (hotkey.rs).
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    hotkey::on_shortcut(
                        app,
                        shortcut,
                        matches!(
                            event.state,
                            tauri_plugin_global_shortcut::ShortcutState::Pressed
                        ),
                    );
                })
                .build(),
        )
        // Launch-at-login (Windows registry Run key). The toggle lives in settings;
        // we enable/disable to match it at startup and on change (settings.rs).
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // Signed in-app auto-update (GitHub Releases + minisign-verified latest.json,
        // ADR-009). `process` provides the relaunch into the freshly-installed version.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            apply_main_window_rounded_corners(app.handle());
            // Load preferences once at startup; failure-safe (defaults on a missing
            // or corrupt file, never a startup failure — settings.rs Rule 4/5).
            // Hydrate the already-managed (default) state from disk now that the
            // app handle exists. Failure-safe (defaults on a missing or corrupt
            // file, never a startup failure — settings.rs Rule 4/5).
            let loaded = settings::load_settings(app.handle());
            let hk_cfg = loaded.hotkey.clone();
            let launch_at_login = loaded.general.launch_at_login;
            app.state::<settings::SettingsState>().hydrate(loaded);
            // Sync the OS autostart entry to the saved preference (best-effort).
            {
                use tauri_plugin_autostart::ManagerExt;
                let mgr = app.autolaunch();
                let _ = if launch_at_login {
                    mgr.enable()
                } else {
                    mgr.disable()
                };
            }
            // Global PTT hotkey runtime + best-effort startup registration (Rule 14).
            app.state::<hotkey::HotkeyRuntime>().hydrate(hk_cfg.clone());
            hotkey::register_initial(app.handle(), &hk_cfg);
            // Self-healing PTT registration (hotkeys.md Rule 15). Windows can silently
            // drop the RegisterHotKey routing (an IME/TSF arming Ctrl+Space, a sleep/lock
            // transition, a brief grab by another app); MIA registers only once above, so
            // without recovery the hotkey stays dead until restart. An idle tick re-claims
            // the chord periodically, and a resume/unlock watcher re-claims it immediately.
            hotkey::start_self_heal(app.handle());
            hotkey::start_windows_key_polling(app.handle());
            power_resume::start(app.handle());
            // Local-only usage stats (never uploaded, ADR-001).
            let stats = stats::load_stats(app.handle());
            app.state::<stats::StatsState>().hydrate(stats);
            // Custom dictionary (personal vocabulary) — loaded from dictionary.json.
            let (dict_entries, dict_settings) = dictionary::load_dictionary(app.handle());
            app.state::<dictionary::DictState>()
                .hydrate(dict_entries, dict_settings);
            let history = history::load_history(app.handle());
            app.state::<history::HistoryState>().hydrate(history);
            // Voice-triggered snippets — loaded from snippets.json.
            let snips = snippets::load_snippets(app.handle());
            app.state::<snippets::SnippetState>().hydrate(snips);
            // System tray (Open / Quit). MIA runs in the tray.
            tray::init(app.handle())?;
            // Dock the floating, click-through, always-on-top mic HUD overlay window
            // (driven by the engine's `hud://state` events — see hud.rs / dictation.rs).
            hud::setup_hud(app.handle());
            Ok(())
        })
        // Close-to-tray: closing the Hub hides it instead of quitting — MIA keeps
        // running in the tray so the global PTT hotkey stays live (tray-and-hud.md).
        // Only the "main" window is intercepted; the click-through HUD is untouched.
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            match event {
                WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    if let Some(stt) = window.app_handle().try_state::<stt::SttState>() {
                        let _ = stt::unload(&stt);
                    }
                    let _ = window.hide();
                }
                // Returning to the Hub is a cheap, natural moment to re-claim the PTT
                // chord if its OS routing was silently dropped (hotkeys.md Rule 15).
                WindowEvent::Focused(true) => hotkey::request_reregister(window.app_handle()),
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            app_version,
            audio::list_input_devices,
            audio::test_microphone,
            audio::open_mic_privacy,
            dictation::start_dictation,
            dictation::stop_dictation,
            dictation::cancel_dictation,
            hotkey::register_hotkey,
            hotkey::unregister_hotkey,
            hotkey::update_hotkey,
            hotkey::get_hotkey,
            hotkey::sample_hotkey_recording,
            dictionary::dict_list,
            dictionary::dict_add,
            dictionary::dict_update,
            dictionary::dict_remove,
            dictionary::dict_settings_get,
            dictionary::dict_settings_set,
            history::list_history,
            history::copy_history_entry,
            history::delete_history_entry,
            history::clear_history,
            inject::inject_text,
            settings::get_settings,
            settings::update_settings,
            settings::reset_settings,
            snippets::list_snippets,
            snippets::upsert_snippet,
            snippets::delete_snippet,
            snippets::preview_expansion,
            stats::get_stats,
            stats::reset_stats,
            stt::list_whisper_models,
            stt::download_whisper_model,
            stt::cancel_whisper_model_download,
            stt::gpu_engine_status,
            stt::download_gpu_engine,
            stt::warm_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
