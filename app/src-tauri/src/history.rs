//! Dictation history: local recovery cache for final dictated text.
//! Stored in `history.json` under app data, never uploaded. The orchestrator records
//! non-empty final text before injection so users can copy it if the target app loses it.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

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
        save_history(app, &list)
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
