//! Dictation history: local recovery cache for final dictated text.
//! Stored in `history.json` under app data, never uploaded. The orchestrator records
//! non-empty final text before injection so users can copy it if the target app loses it.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

const MAX_HISTORY_ITEMS: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub text: String,
    pub created_at_ms: u64,
    pub word_count: u64,
}

pub struct HistoryState {
    inner: Mutex<Vec<HistoryEntry>>,
}

impl HistoryState {
    pub fn new(entries: Vec<HistoryEntry>) -> Self {
        Self {
            inner: Mutex::new(trim_history(entries, MAX_HISTORY_ITEMS)),
        }
    }

    pub fn hydrate(&self, entries: Vec<HistoryEntry>) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = trim_history(entries, MAX_HISTORY_ITEMS);
        }
    }

    fn list(&self) -> Result<Vec<HistoryEntry>, String> {
        Ok(self
            .inner
            .lock()
            .map_err(|_| "history state poisoned".to_string())?
            .clone())
    }

    fn set(&self, entries: Vec<HistoryEntry>) -> Result<(), String> {
        *self
            .inner
            .lock()
            .map_err(|_| "history state poisoned".to_string())? =
            trim_history(entries, MAX_HISTORY_ITEMS);
        Ok(())
    }

    pub fn record_and_save(&self, app: &AppHandle, text: &str) -> Result<(), String> {
        let Some(entry) = new_entry(text) else {
            return Ok(());
        };
        let mut list = self
            .inner
            .lock()
            .map_err(|_| "history state poisoned".to_string())?;
        list.insert(0, entry);
        list.truncate(MAX_HISTORY_ITEMS);
        save_history(app, &list)?;
        let _ = app.emit("history://saved", ());
        Ok(())
    }
}

fn history_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("history.json"))
}

pub fn load_history(app: &AppHandle) -> Vec<HistoryEntry> {
    let Ok(path) = history_path(app) else {
        return Vec::new();
    };
    trim_history(
        crate::persist::load_json_or_default(&path),
        MAX_HISTORY_ITEMS,
    )
}

fn save_history(app: &AppHandle, entries: &[HistoryEntry]) -> Result<(), String> {
    crate::persist::atomic_write_json(&history_path(app)?, &entries)
}

fn new_entry(text: &str) -> Option<HistoryEntry> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return None;
    }
    Some(HistoryEntry {
        id: crate::persist::new_id("hist-"),
        word_count: crate::stats::count_words(&text),
        created_at_ms: crate::persist::now_ms(),
        text,
    })
}

fn trim_history(mut entries: Vec<HistoryEntry>, max: usize) -> Vec<HistoryEntry> {
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.created_at_ms));
    entries.truncate(max);
    entries
}

fn delete_entry(entries: &[HistoryEntry], id: &str) -> Vec<HistoryEntry> {
    entries.iter().filter(|e| e.id != id).cloned().collect()
}

#[tauri::command]
pub fn list_history(state: State<'_, HistoryState>) -> Result<Vec<HistoryEntry>, String> {
    state.list()
}

#[tauri::command]
pub fn copy_history_entry(state: State<'_, HistoryState>, id: String) -> Result<(), String> {
    let text = state
        .list()?
        .into_iter()
        .find(|e| e.id == id)
        .map(|e| e.text)
        .ok_or_else(|| "history entry not found".to_string())?;
    arboard::Clipboard::new()
        .map_err(|_| "clipboard unavailable".to_string())?
        .set_text(text)
        .map_err(|_| "clipboard unavailable".to_string())
}

#[tauri::command]
pub fn delete_history_entry(
    app: AppHandle,
    state: State<'_, HistoryState>,
    id: String,
) -> Result<(), String> {
    let next = delete_entry(&state.list()?, &id);
    save_history(&app, &next)?;
    state.set(next)
}

#[tauri::command]
pub fn clear_history(app: AppHandle, state: State<'_, HistoryState>) -> Result<(), String> {
    let cleared = Vec::new();
    save_history(&app, &cleared)?;
    state.set(cleared)
}

/// Snapshot written by `export_history` — the newest-first entries plus enough
/// metadata (app version, export instant) to make the file self-describing when
/// attached to a bug report.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryExport {
    app_version: String,
    exported_at_ms: u64,
    entries: Vec<HistoryEntry>,
}

/// Export the whole history as a JSON file in the user's Downloads folder and
/// reveal it in Explorer, so transcripts can be shared/attached to a bug report
/// without digging through AppData. Local file only — nothing is uploaded
/// (ADR-001). Returns the written path for the Hub to display.
#[tauri::command]
pub fn export_history(app: AppHandle, state: State<'_, HistoryState>) -> Result<String, String> {
    let entries = state.list()?;
    if entries.is_empty() {
        return Err("history is empty".to_string());
    }
    let dir = app
        .path()
        .download_dir()
        .map_err(|e| format!("could not resolve the Downloads folder: {e}"))?;
    let path = dir.join(format!("mia-history-{}.json", utc_stamp(crate::persist::now_secs())));
    let export = HistoryExport {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        exported_at_ms: crate::persist::now_ms(),
        entries,
    };
    crate::persist::atomic_write_json(&path, &export)?;
    reveal_in_explorer(&path);
    Ok(path.to_string_lossy().into_owned())
}

/// Best-effort `explorer /select` so the freshly written export is one click from
/// being dragged into a chat or issue. Failure is ignored — the path is returned
/// to the Hub either way.
fn reveal_in_explorer(path: &Path) {
    let _ = std::process::Command::new("explorer.exe")
        .arg(format!("/select,{}", path.display()))
        .spawn();
}

/// UTC `YYYYMMDD-HHMMSS` for the export filename. WHY hand-rolled: it avoids a
/// chrono dependency for one filename — the civil-date math is Hinnant's
/// days-from-epoch algorithm, unit-tested below.
fn utc_stamp(secs: u64) -> String {
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}{month:02}{day:02}-{h:02}{m:02}{s:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, text: &str, created_at_ms: u64) -> HistoryEntry {
        HistoryEntry {
            id: id.to_string(),
            text: text.to_string(),
            created_at_ms,
            word_count: crate::stats::count_words(text),
        }
    }

    #[test]
    fn new_entry_trims_and_skips_empty_text() {
        assert!(new_entry("   ").is_none());
        let e = new_entry("  ola mundo  ").unwrap();
        assert_eq!(e.text, "ola mundo");
        assert_eq!(e.word_count, 2);
        assert!(e.id.starts_with("hist-"));
    }

    #[test]
    fn trim_history_sorts_newest_first_and_caps() {
        let entries = vec![
            entry("old", "a", 1),
            entry("new", "b", 3),
            entry("mid", "c", 2),
        ];
        let trimmed = trim_history(entries, 2);
        assert_eq!(
            trimmed.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            vec!["new", "mid"]
        );
    }

    #[test]
    fn utc_stamp_formats_known_instants() {
        assert_eq!(utc_stamp(0), "19700101-000000");
        // 2001-09-09 01:46:40 UTC — the classic 1e9 epoch instant.
        assert_eq!(utc_stamp(1_000_000_000), "20010909-014640");
        // 2000-02-29 12:00:00 UTC — a leap day crossing the era boundary.
        assert_eq!(utc_stamp(951_825_600), "20000229-120000");
    }

    #[test]
    fn delete_entry_is_idempotent() {
        let entries = vec![entry("a", "one", 1), entry("b", "two", 2)];
        let next = delete_entry(&entries, "a");
        assert_eq!(
            next.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            vec!["b"]
        );
        assert_eq!(delete_entry(&next, "missing"), next);
    }
}
