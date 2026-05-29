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
pub mod dictation;
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
        .manage(audio::CaptureState::default())
        // Global push-to-talk: the plugin delivers key edges; the handler runs the
        // pure reducer and emits `dictation://intent` for the frontend (hotkey.rs).
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    hotkey::on_shortcut_event(
                        app,
                        matches!(event.state, tauri_plugin_global_shortcut::ShortcutState::Pressed),
                    );
                })
                .build(),
        )
        .setup(|app| {
            // Load preferences once at startup; failure-safe (defaults on a missing
            // or corrupt file, never a startup failure — settings.rs Rule 4/5).
            let loaded = settings::load_settings(app.handle());
            let hk_cfg = loaded.hotkey.clone();
            app.manage(settings::SettingsState::new(loaded));
            // Global PTT hotkey runtime + best-effort startup registration (Rule 14).
            app.manage(hotkey::HotkeyRuntime::new(hk_cfg.clone()));
            hotkey::register_initial(app.handle(), &hk_cfg);
            // Local-only usage stats (never uploaded, ADR-001).
            let stats = stats::load_stats(app.handle());
            app.manage(stats::StatsState::new(stats));
            // Custom dictionary (personal vocabulary) — loaded from dictionary.json.
            let (dict_entries, dict_settings) = dictionary::load_dictionary(app.handle());
            app.manage(dictionary::DictState::new(dict_entries, dict_settings));
            // Voice-triggered snippets — loaded from snippets.json.
            let snips = snippets::load_snippets(app.handle());
            app.manage(snippets::SnippetState::new(snips));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_version,
            audio::list_input_devices,
            audio::test_microphone,
            dictation::start_dictation,
            dictation::stop_dictation,
            dictation::cancel_dictation,
            hotkey::register_hotkey,
            hotkey::unregister_hotkey,
            hotkey::update_hotkey,
            hotkey::get_hotkey,
            dictionary::dict_list,
            dictionary::dict_add,
            dictionary::dict_update,
            dictionary::dict_remove,
            dictionary::dict_settings_get,
            dictionary::dict_settings_set,
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
            stt::gpu_engine_status,
            stt::download_gpu_engine,
            stt::warm_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
