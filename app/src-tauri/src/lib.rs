//! MIA — local voice-to-text dictation for Windows.
//!
//! Rust is the engine (see `CLAUDE.md` and `docs/specs/architecture.md`). This
//! file owns the Tauri bootstrap + the `#[tauri::command]` registry; the
//! dictation modules (audio, vad, stt, cleanup, inject, hotkey, tray, hud,
//! dictation) are wired in here as each Phase-1 stage lands. So far the engine
//! exposes model management + warm-status (stt), text injection (inject), and the
//! `app_version` IPC smoke test; cleanup is a pure module called in-process.

pub mod cleanup;
pub mod inject;
pub mod stt;

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
        .invoke_handler(tauri::generate_handler![
            app_version,
            inject::inject_text,
            stt::list_whisper_models,
            stt::download_whisper_model,
            stt::gpu_engine_status,
            stt::download_gpu_engine,
            stt::warm_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
