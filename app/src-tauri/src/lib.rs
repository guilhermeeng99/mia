//! MIA — local voice-to-text dictation for Windows.
//!
//! Rust is the engine (see `CLAUDE.md` and `docs/specs/architecture.md`). This
//! file owns the Tauri bootstrap + the `#[tauri::command]` registry; the
//! dictation modules (audio, vad, stt, cleanup, inject, hotkey, tray, hud,
//! dictation) are wired in here as each Phase-1 stage lands. So far the engine
//! exposes model management + warm-status (stt), text injection (inject), and the
//! `app_version` IPC smoke test; cleanup is a pure module called in-process.

pub mod ai_commands;
pub mod audio;
pub mod cleanup;
pub mod dictionary;
pub mod hotkey;
pub mod inject;
pub mod settings;
pub mod snippets;
pub mod stats;
pub mod stt;
pub mod vad;

use tauri::Manager;

/// Return the running app version (compiled in from Cargo). Trivial by design —
/// it is the scaffold's IPC smoke test, called by `App.svelte`.
#[tauri::command]
fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // The warm whisper-server lives once in managed state, shared across
        // every utterance (ADR-004) — never a cold spawn per utterance.
        .manage(stt::SttState::default())
        .setup(|app| {
            // Load preferences once at startup; failure-safe (defaults on a missing
            // or corrupt file, never a startup failure — settings.rs Rule 4/5).
            let loaded = settings::load_settings(app.handle());
            app.manage(settings::SettingsState::new(loaded));
            // Local-only usage stats (never uploaded, ADR-001).
            let stats = stats::load_stats(app.handle());
            app.manage(stats::StatsState::new(stats));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_version,
            audio::list_input_devices,
            inject::inject_text,
            settings::get_settings,
            settings::update_settings,
            settings::reset_settings,
            stats::get_stats,
            stats::reset_stats,
            stt::list_whisper_models,
            stt::download_whisper_model,
            stt::gpu_engine_status,
            stt::download_gpu_engine,
            stt::warm_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
