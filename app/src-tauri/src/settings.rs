//! User preferences + persistence (ADR-006, Phase 1) — the single `settings.json`
//! source of truth under the OS app-config dir. See `docs/specs/settings.md`.
//!
//! The decode/merge/validate/migrate logic is **pure and cargo-tested**
//! (`Settings::default`, `apply_patch`, `validate`, `migrate`, `parse_settings`);
//! the load/save commands wrap it with atomic file I/O. Load is **failure-safe**: a
//! missing file → defaults, a corrupt file → defaults + sidelined backup, never a
//! startup failure (Rule 5). The side effects `update_settings` should apply
//! (re-register hotkey, swap warm model, launch-at-login) are wired as those
//! runtime subsystems land; for now it validates + persists.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

/// Bumped when the on-disk shape changes; `migrate` upgrades older files (Rule 6).
pub const SCHEMA_VERSION: u32 = 1;
/// Bundled small CPU model — the latency-first default (§4; see `speech-to-text.md`).
const DEFAULT_MODEL_ID: &str = "small";
/// Sentinel for "follow the OS default input device" (Rule 11).
const DEFAULT_DEVICE: &str = "default";

fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

// ─────────────────────────────────────────────────────────────────────────────
// The preferences tree (§4) — serde camelCase, every group defaults independently.
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum DefaultLanguage {
    #[default]
    Auto,
    Pt,
    En,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum Engine {
    #[default]
    Cpu,
    Cuda,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum HudPosition {
    #[default]
    Caret,
    BottomCenter,
    BottomRight,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AiModel {
    #[default]
    #[serde(rename = "qwen2.5-3b")]
    Qwen25_3b,
    #[serde(rename = "llama-3.2-3b")]
    Llama32_3b,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct GeneralSettings {
    pub launch_at_login: bool,
    pub dictation_enabled: bool,
    pub default_language: DefaultLanguage,
    pub play_sounds: bool,
    pub collect_stats: bool,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            launch_at_login: false,
            dictation_enabled: true,
            default_language: DefaultLanguage::Auto,
            play_sounds: false,
            collect_stats: true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct ModelSettings {
    pub model: String,
    pub engine: Engine,
    pub unload_on_idle: bool,
}

impl Default for ModelSettings {
    fn default() -> Self {
        Self { model: DEFAULT_MODEL_ID.to_string(), engine: Engine::Cpu, unload_on_idle: true }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct AudioSettings {
    pub input_device: String,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self { input_device: DEFAULT_DEVICE.to_string() }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct CleanupSettings {
    pub filler_removal: bool,
    pub spoken_punctuation: bool,
    pub stutter_collapse: bool,
    pub capitalization: bool,
}

impl Default for CleanupSettings {
    fn default() -> Self {
        Self {
            filler_removal: true,
            spoken_punctuation: true,
            stutter_collapse: true,
            capitalization: true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct HudSettings {
    pub position: HudPosition,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AiSettings {
    pub enabled: bool,
    pub model: AiModel,
    pub polish_on_insert: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdatesSettings {
    pub auto_check_updates: bool,
}

impl Default for UpdatesSettings {
    fn default() -> Self {
        Self { auto_check_updates: true }
    }
}

/// The full preferences tree (Rule 1). `schemaVersion` drives migration (Rule 6).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub general: GeneralSettings,
    #[serde(default)]
    pub hotkey: crate::hotkey::HotkeyConfig,
    #[serde(default)]
    pub model: ModelSettings,
    #[serde(default)]
    pub audio: AudioSettings,
    #[serde(default)]
    pub cleanup: CleanupSettings,
    #[serde(default)]
    pub hud: HudSettings,
    #[serde(default)]
    pub ai: AiSettings,
    #[serde(default)]
    pub updates: UpdatesSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            general: GeneralSettings::default(),
            hotkey: crate::hotkey::HotkeyConfig::default(),
            model: ModelSettings::default(),
            audio: AudioSettings::default(),
            cleanup: CleanupSettings::default(),
            hud: HudSettings::default(),
            ai: AiSettings::default(),
            updates: UpdatesSettings::default(),
        }
    }
}

/// A merge-patch: only present groups change (Rule 3). Group-granular (the UI sends
/// the whole group it edited) — coarser than per-field but avoids round-tripping the
/// whole tree and keeps the merge total and testable.
#[derive(Deserialize, Default, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct SettingsPatch {
    pub general: Option<GeneralSettings>,
    pub hotkey: Option<crate::hotkey::HotkeyConfig>,
    pub model: Option<ModelSettings>,
    pub audio: Option<AudioSettings>,
    pub cleanup: Option<CleanupSettings>,
    pub hud: Option<HudSettings>,
    pub ai: Option<AiSettings>,
    pub updates: Option<UpdatesSettings>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Pure core (cargo-tested, no I/O)
// ─────────────────────────────────────────────────────────────────────────────

/// Apply a patch to a base, then re-validate (Rule 3).
pub fn apply_patch(base: &Settings, patch: &SettingsPatch) -> Settings {
    let mut s = base.clone();
    if let Some(g) = &patch.general {
        s.general = g.clone();
    }
    if let Some(h) = &patch.hotkey {
        s.hotkey = h.clone();
    }
    if let Some(m) = &patch.model {
        s.model = m.clone();
    }
    if let Some(a) = &patch.audio {
        s.audio = a.clone();
    }
    if let Some(c) = &patch.cleanup {
        s.cleanup = *c;
    }
    if let Some(h) = &patch.hud {
        s.hud = *h;
    }
    if let Some(a) = &patch.ai {
        s.ai = *a;
    }
    if let Some(u) = &patch.updates {
        s.updates = *u;
    }
    validate(s)
}

/// Clamp / normalize defensively — the UI is never the only guard (Rule 3, §4). An
/// unparseable hotkey or empty device/model falls back to its default.
pub fn validate(mut s: Settings) -> Settings {
    s.schema_version = SCHEMA_VERSION;
    if crate::hotkey::parse_accelerator(&s.hotkey.accelerator).is_err() {
        s.hotkey.accelerator = crate::hotkey::DEFAULT_ACCEL.to_string();
    }
    if s.audio.input_device.trim().is_empty() {
        s.audio.input_device = DEFAULT_DEVICE.to_string();
    }
    if s.model.model.trim().is_empty() {
        s.model.model = DEFAULT_MODEL_ID.to_string();
    }
    s
}

/// Upgrade an older on-disk JSON shape in memory (Rule 6). For v1 this just ensures
/// `schemaVersion` is present/current; field-shape upgrades slot in here later.
fn migrate(mut value: serde_json::Value) -> serde_json::Value {
    if let Some(obj) = value.as_object_mut() {
        let ver = obj.get("schemaVersion").and_then(|v| v.as_u64()).unwrap_or(0);
        if ver < SCHEMA_VERSION as u64 {
            obj.insert("schemaVersion".to_string(), serde_json::json!(SCHEMA_VERSION));
        }
    }
    value
}

/// Parse raw `settings.json` text → migrate → deserialize (missing groups default)
/// → validate. Pure; the loader maps an `Err` here to the corrupt-file path (Rule 5).
pub fn parse_settings(raw: &str) -> Result<Settings, String> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("settings parse error: {e}"))?;
    let migrated = migrate(value);
    let parsed: Settings =
        serde_json::from_value(migrated).map_err(|e| format!("settings shape error: {e}"))?;
    Ok(validate(parsed))
}

// ─────────────────────────────────────────────────────────────────────────────
// Persistence + managed state + commands
// ─────────────────────────────────────────────────────────────────────────────

/// The in-memory authoritative copy, loaded once at startup (Rule 2).
pub struct SettingsState {
    inner: Mutex<Settings>,
}

impl SettingsState {
    pub fn new(settings: Settings) -> Self {
        Self { inner: Mutex::new(settings) }
    }

    fn get(&self) -> Result<Settings, String> {
        Ok(self.inner.lock().map_err(|_| "settings state poisoned".to_string())?.clone())
    }

    fn set(&self, settings: Settings) -> Result<(), String> {
        *self.inner.lock().map_err(|_| "settings state poisoned".to_string())? = settings;
        Ok(())
    }
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_config_dir().map_err(|e| e.to_string())?.join("settings.json"))
}

fn map_write_err(e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        "settings file is read-only or locked".to_string()
    } else {
        format!("could not write settings: {e}")
    }
}

/// Move an unparseable file aside so the app starts clean on defaults (Rule 5).
fn sideline_corrupt(path: &Path) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let backup = path.with_file_name(format!("settings.corrupt-{ts}.json"));
    let _ = std::fs::rename(path, backup);
}

/// Failure-safe load (Rule 4/5): missing → defaults; corrupt → defaults + backup.
pub fn load_settings(app: &AppHandle) -> Settings {
    let Ok(path) = settings_path(app) else {
        return Settings::default();
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Settings::default(); // missing file is first-run, not an error
    };
    match parse_settings(&raw) {
        Ok(settings) => settings,
        Err(_) => {
            sideline_corrupt(&path);
            Settings::default()
        }
    }
}

/// Atomic write: serialize → temp file → rename over `settings.json` (Rule 3).
fn save_settings(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = settings_path(app)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(map_write_err)?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(map_write_err)?;
    std::fs::rename(&tmp, &path).map_err(map_write_err)
}

/// Return the in-memory settings (loaded once at startup).
#[tauri::command]
pub fn get_settings(state: State<'_, SettingsState>) -> Result<Settings, String> {
    state.get()
}

/// Merge a patch, validate, persist atomically, update the in-memory copy. Side
/// effects (hotkey re-register, warm-model swap, launch-at-login) land as those
/// subsystems are wired (§2) — for now this validates + persists.
#[tauri::command]
pub fn update_settings(
    app: AppHandle,
    state: State<'_, SettingsState>,
    patch: SettingsPatch,
) -> Result<Settings, String> {
    let next = apply_patch(&state.get()?, &patch);
    save_settings(&app, &next)?;
    state.set(next.clone())?;
    Ok(next)
}

/// Overwrite with defaults; persist; update the in-memory copy ("Reset to defaults").
#[tauri::command]
pub fn reset_settings(
    app: AppHandle,
    state: State<'_, SettingsState>,
) -> Result<Settings, String> {
    let next = Settings::default();
    save_settings(&app, &next)?;
    state.set(next.clone())?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let s = Settings::default();
        assert_eq!(s.schema_version, SCHEMA_VERSION);
        assert_eq!(s.general.default_language, DefaultLanguage::Auto);
        assert!(s.general.dictation_enabled);
        assert!(!s.general.launch_at_login);
        assert!(s.general.collect_stats);
        assert_eq!(s.hotkey.accelerator, "Ctrl+Space");
        assert_eq!(s.model.model, "small");
        assert_eq!(s.model.engine, Engine::Cpu);
        assert!(s.model.unload_on_idle);
        assert_eq!(s.audio.input_device, "default");
        assert!(s.cleanup.filler_removal && s.cleanup.capitalization);
        assert_eq!(s.hud.position, HudPosition::Caret);
        assert!(!s.ai.enabled);
        assert!(s.updates.auto_check_updates);
    }

    #[test]
    fn apply_patch_merges_only_present_groups() {
        let base = Settings::default();
        let patch = SettingsPatch {
            general: Some(GeneralSettings { launch_at_login: true, ..Default::default() }),
            ..Default::default()
        };
        let next = apply_patch(&base, &patch);
        assert!(next.general.launch_at_login); // changed
        assert_eq!(next.model, base.model); // untouched
        assert_eq!(next.audio, base.audio); // untouched
    }

    #[test]
    fn validate_resets_bad_accelerator_and_empty_device() {
        let mut s = Settings::default();
        s.hotkey.accelerator = "not a chord %%%".to_string();
        s.audio.input_device = "   ".to_string();
        s.model.model = "".to_string();
        let v = validate(s);
        assert_eq!(v.hotkey.accelerator, "Ctrl+Space");
        assert_eq!(v.audio.input_device, "default");
        assert_eq!(v.model.model, "small");
    }

    #[test]
    fn migrate_inserts_missing_schema_version() {
        let v = serde_json::json!({ "general": { "playSounds": true } });
        let migrated = migrate(v);
        assert_eq!(migrated.get("schemaVersion").and_then(|x| x.as_u64()), Some(1));
    }

    #[test]
    fn parse_round_trips_default() {
        let json = serde_json::to_string(&Settings::default()).unwrap();
        assert_eq!(parse_settings(&json).unwrap(), Settings::default());
    }

    #[test]
    fn parse_fills_missing_groups_with_defaults() {
        // A partial file (only schemaVersion) must load, not fail (Rule 6 tolerance).
        let s = parse_settings(r#"{ "schemaVersion": 1 }"#).unwrap();
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn parse_tolerates_missing_schema_version() {
        let s = parse_settings(r#"{ "general": { "playSounds": true } }"#).unwrap();
        assert_eq!(s.schema_version, SCHEMA_VERSION);
        assert!(s.general.play_sounds);
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(parse_settings("}{ not json").is_err());
    }

    #[test]
    fn ai_model_serializes_with_dotted_names() {
        let json = serde_json::to_string(&AiModel::Qwen25_3b).unwrap();
        assert_eq!(json, "\"qwen2.5-3b\"");
        let back: AiModel = serde_json::from_str("\"llama-3.2-3b\"").unwrap();
        assert_eq!(back, AiModel::Llama32_3b);
    }
}
