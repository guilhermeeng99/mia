//! MIA — local voice-to-text dictation for Windows.
//!
//! Rust is the engine (see `CLAUDE.md` and `docs/specs/architecture.md`). This
//! file owns the Tauri bootstrap + the `#[tauri::command]` registry; the
//! dictation modules (audio, vad, stt, cleanup, inject, hotkey, tray, hud,
//! dictation) are wired in here as each Phase-1 stage lands. The scaffold
//! exposes only `app_version`, a trivial command that proves the Svelte ↔ Rust
//! IPC bridge end-to-end.

pub mod cleanup;

/// Return the running app version (compiled in from Cargo). Trivial by design —
/// it is the scaffold's IPC smoke test, called by `App.svelte`.
#[tauri::command]
fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![app_version])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
