//! Shared JSON persistence helpers — the one place that knows how to write a config
//! file atomically and to load one failure-safe. Every per-feature module
//! (settings, dictionary, snippets, stats) persists through here so the
//! create-dir → temp-file → rename dance and the missing/corrupt-file fallback live
//! in a single, cargo-tested spot instead of being copy-pasted four times.
//!
//! WHY atomic: a half-written `.json` (process killed mid-write) must never replace
//! a good file. We write a sibling `.tmp` and `rename` it over the target — on every
//! OS `rename` over an existing file is atomic, so a reader sees either the old file
//! or the complete new one, never a truncated mix.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Serialize `value` and atomically replace `path`: create the parent dir, write a
/// sibling `.tmp`, then `rename` it over the target (Rule 3 across all stores).
pub fn atomic_write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Failure-safe load: a missing or unparseable file yields `T::default()`, never an
/// error. WHY discard-on-corrupt: secondary stores (dictionary/snippets/stats) would
/// rather start empty than block startup. The *primary* `settings.json` deliberately
/// does NOT use this (it sidelines a corrupt file instead — see `settings.rs`).
pub fn load_json_or_default<T: serde::de::DeserializeOwned + Default>(path: &Path) -> T {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Process-lifetime counter so two ids minted in the same nanosecond still differ.
static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Wall-clock milliseconds since the Unix epoch (saturating to 0 on a pre-epoch clock).
/// The one shared time stamp so the hotkey debounce, the dictation latency math, and
/// any future timestamp all derive it the same way instead of re-implementing it (DRY).
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Wall-clock seconds since the Unix epoch (saturating to 0). Used for the day-streak
/// math and the corrupt-settings backup filename timestamp.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A unique id `"{prefix}{nanos}-{counter}"`. WHY the counter suffix: a coarse clock
/// can return identical nanos for back-to-back inserts; the atomic counter guarantees
/// uniqueness within a single process run regardless of clock resolution.
pub fn new_id(prefix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let counter = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}{nanos}-{counter}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Default, PartialEq, Debug)]
    struct Sample {
        name: String,
        count: u32,
    }

    fn temp_path(name: &str) -> std::path::PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("mia-persist-test-{}-{}", std::process::id(), name));
        dir.push("file.json");
        dir
    }

    #[test]
    fn atomic_write_then_load_round_trips() {
        let path = temp_path("roundtrip");
        let value = Sample { name: "mia".to_string(), count: 7 };
        atomic_write_json(&path, &value).unwrap();
        let back: Sample = load_json_or_default(&path);
        assert_eq!(back, value);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn load_missing_file_is_default() {
        let path = temp_path("missing");
        let back: Sample = load_json_or_default(&path);
        assert_eq!(back, Sample::default());
    }

    #[test]
    fn load_corrupt_file_is_default() {
        let path = temp_path("corrupt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "}{ not json").unwrap();
        let back: Sample = load_json_or_default(&path);
        assert_eq!(back, Sample::default());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn now_helpers_are_post_epoch_and_consistent() {
        // now_secs is read first, now_ms after, so now_ms's instant is >= now_secs's.
        let secs = now_secs();
        assert!(secs > 1_700_000_000); // comfortably after 2023, so the clock is sane
        assert!(now_ms() >= secs * 1000); // ms granularity at the same-or-later instant
    }

    #[test]
    fn new_id_is_unique_across_rapid_calls_and_keeps_prefix() {
        let a = new_id("dict-");
        let b = new_id("dict-");
        assert_ne!(a, b); // distinct even if the clock didn't advance
        assert!(a.starts_with("dict-"));
        assert!(b.starts_with("dict-"));
    }
}
