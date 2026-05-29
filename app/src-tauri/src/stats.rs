//! Local-only usage stats (ADR-001, settings.md §6/Rule 15) — words dictated,
//! average words-per-minute, and a day streak, stored in `stats.json` in the app
//! data dir. **Never uploaded; no telemetry.** A `collectStats` toggle and
//! `reset_stats` govern collection (settings.rs `GeneralSettings.collect_stats`).
//!
//! The WPM + streak arithmetic and word counting are **pure and cargo-tested**;
//! the commands wrap them with the same atomic-write persistence as settings.rs.
//! `record_dictation` is called by the orchestrator after each successful insert
//! (runtime-pending until the dictation loop lands).

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

/// Persisted counters (serde camelCase). `last_dictation_day` is days-since-epoch
/// (0 = never), so streak math is pure integer arithmetic.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct UsageStats {
    pub total_words: u64,
    pub total_ms: u64,
    pub day_streak: u32,
    pub best_streak: u32,
    pub last_dictation_day: i64,
}

/// What the Hub dashboard renders — the stored counters plus a derived average WPM.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UsageStatsView {
    pub total_words: u64,
    pub total_ms: u64,
    pub avg_wpm: u32,
    pub day_streak: u32,
    pub best_streak: u32,
}

// ─────────────────────────────────────────────────────────────────────────────
// Pure core (cargo-tested)
// ─────────────────────────────────────────────────────────────────────────────

/// Words per minute over `ms` milliseconds. Zero time → 0 (no division by zero).
pub fn wpm(words: u64, ms: u64) -> u32 {
    if ms == 0 {
        return 0;
    }
    ((words as f64) / (ms as f64 / 60_000.0)).round() as u32
}

/// Count whitespace-delimited words in a transcript.
pub fn count_words(text: &str) -> u64 {
    text.split_whitespace().count() as u64
}

/// The day streak after a dictation on `today` (days-since-epoch). Same day keeps
/// the streak, the next day extends it, a gap resets to 1, and the first-ever
/// dictation (`last_day == 0`) starts at 1.
pub fn next_streak(last_day: i64, today: i64, current: u32) -> u32 {
    if last_day == 0 {
        return 1; // first dictation
    }
    match today - last_day {
        0 => current.max(1), // same day, unchanged (but never 0 after a dictation)
        1 => current + 1,    // consecutive day
        _ => 1,              // gap (or clock moved back) → restart
    }
}

/// Fold one successful dictation into the counters (call after a real insert).
pub fn record_dictation(mut stats: UsageStats, words: u64, ms: u64, today: i64) -> UsageStats {
    stats.total_words += words;
    stats.total_ms += ms;
    stats.day_streak = next_streak(stats.last_dictation_day, today, stats.day_streak);
    stats.best_streak = stats.best_streak.max(stats.day_streak);
    stats.last_dictation_day = today;
    stats
}

/// Project the stored counters into the dashboard view (adds derived WPM).
pub fn view(stats: &UsageStats) -> UsageStatsView {
    UsageStatsView {
        total_words: stats.total_words,
        total_ms: stats.total_ms,
        avg_wpm: wpm(stats.total_words, stats.total_ms),
        day_streak: stats.day_streak,
        best_streak: stats.best_streak,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Persistence + managed state + commands
// ─────────────────────────────────────────────────────────────────────────────

/// The in-memory counters (loaded once at startup). Stats live in `stats.json` in
/// the app **data** dir — separate from `settings.json` so wiping stats never
/// touches preferences (Rule 1).
pub struct StatsState {
    inner: Mutex<UsageStats>,
}

impl StatsState {
    pub fn new(stats: UsageStats) -> Self {
        Self { inner: Mutex::new(stats) }
    }

    fn get(&self) -> Result<UsageStats, String> {
        Ok(*self.inner.lock().map_err(|_| "stats state poisoned".to_string())?)
    }

    fn set(&self, stats: UsageStats) -> Result<(), String> {
        *self.inner.lock().map_err(|_| "stats state poisoned".to_string())? = stats;
        Ok(())
    }

    /// Fold one dictation into the counters and persist (called by the orchestrator).
    pub fn record_and_save(
        &self,
        app: &AppHandle,
        words: u64,
        ms: u64,
        today: i64,
    ) -> Result<(), String> {
        let updated = record_dictation(self.get()?, words, ms, today);
        save_stats(app, &updated)?;
        self.set(updated)
    }
}

fn stats_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("stats.json"))
}

/// Failure-safe load: missing or unparseable → zeroed stats (never a startup error).
pub fn load_stats(app: &AppHandle) -> UsageStats {
    let Ok(path) = stats_path(app) else {
        return UsageStats::default();
    };
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_stats(app: &AppHandle, stats: &UsageStats) -> Result<(), String> {
    let path = stats_path(app)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(stats).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

/// The Hub dashboard view (totals + derived WPM + streak).
#[tauri::command]
pub fn get_stats(state: State<'_, StatsState>) -> Result<UsageStatsView, String> {
    Ok(view(&state.get()?))
}

/// Clear local stats and persist the cleared file (Rule 15).
#[tauri::command]
pub fn reset_stats(app: AppHandle, state: State<'_, StatsState>) -> Result<(), String> {
    let cleared = UsageStats::default();
    save_stats(&app, &cleared)?;
    state.set(cleared)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wpm_basic_and_zero() {
        assert_eq!(wpm(150, 60_000), 150); // 150 words in 1 min
        assert_eq!(wpm(75, 30_000), 150); // 75 words in 30 s → 150 wpm
        assert_eq!(wpm(0, 0), 0); // no division by zero
        assert_eq!(wpm(10, 0), 0);
    }

    #[test]
    fn count_words_splits_on_whitespace() {
        assert_eq!(count_words("  olá   mundo bonito \n tchau "), 4);
        assert_eq!(count_words(""), 0);
    }

    #[test]
    fn streak_starts_continues_resets() {
        assert_eq!(next_streak(0, 100, 0), 1); // first ever
        assert_eq!(next_streak(100, 100, 3), 3); // same day, unchanged
        assert_eq!(next_streak(100, 101, 3), 4); // consecutive
        assert_eq!(next_streak(100, 105, 3), 1); // gap resets
        assert_eq!(next_streak(100, 90, 3), 1); // clock moved back → restart
    }

    #[test]
    fn record_accumulates_and_tracks_best_streak() {
        let s = UsageStats::default();
        let s = record_dictation(s, 20, 10_000, 100);
        assert_eq!(s.total_words, 20);
        assert_eq!(s.day_streak, 1);
        assert_eq!(s.best_streak, 1);
        let s = record_dictation(s, 30, 10_000, 101); // next day
        assert_eq!(s.total_words, 50);
        assert_eq!(s.day_streak, 2);
        assert_eq!(s.best_streak, 2);
        let s = record_dictation(s, 10, 5_000, 110); // gap → streak resets, best kept
        assert_eq!(s.day_streak, 1);
        assert_eq!(s.best_streak, 2);
    }

    #[test]
    fn view_adds_derived_wpm() {
        let s = record_dictation(UsageStats::default(), 100, 60_000, 1);
        let v = view(&s);
        assert_eq!(v.total_words, 100);
        assert_eq!(v.avg_wpm, 100);
        assert_eq!(v.day_streak, 1);
    }
}
