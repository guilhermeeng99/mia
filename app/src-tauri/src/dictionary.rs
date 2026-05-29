//! Custom dictionary (personal vocabulary) — deterministic post-transcription
//! replacement (ADR-008, Phase 3). Runs in the text stage alongside `cleanup.rs`:
//! it rewrites the cleaned transcript so personal terms come out spelled/cased the
//! way the user wants (`mia` → `MIA`, `react js` → `React`). See
//! `docs/specs/custom-dictionary.md`.
//!
//! The matcher is the **pure, cargo-tested core** (mechanism a + the bias-prompt
//! composer for mechanism b). It is token-based, so whole-word matching is
//! inherent; sub-word matching (`wholeWord=false`) is deferred. The CRUD commands +
//! `dictionary.json` persistence + the managed `State` are below; wiring the bias
//! prompt into the warm-Whisper call is the orchestrator's job.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

/// Fuzzy matching is skipped for variants this short or shorter — protects common
/// small words from being clobbered (Rule 8).
const FUZZY_MIN_LEN: usize = 3;
/// Defensive cap on a replacement's length (Rule 12 / §2 error).
const MAX_REPLACEMENT_CHARS: usize = 200;

/// One personal-vocabulary entry (§2). Defaults match §4.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct DictEntry {
    pub id: String,
    pub replacement: String,
    pub sounds_like: Vec<String>,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub fuzzy: bool,
    pub bias_prompt: bool,
    pub enabled: bool,
}

impl Default for DictEntry {
    fn default() -> Self {
        Self {
            id: String::new(),
            replacement: String::new(),
            sounds_like: Vec::new(),
            case_sensitive: false,
            whole_word: true,
            fuzzy: false,
            bias_prompt: true,
            enabled: true,
        }
    }
}

/// Global dictionary toggles (§2/§4).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct DictSettings {
    pub fuzzy_enabled_globally: bool,
    pub fuzzy_max_distance: u8,
    pub bias_enabled: bool,
    pub bias_max_terms: u16,
}

impl Default for DictSettings {
    fn default() -> Self {
        Self {
            fuzzy_enabled_globally: true,
            fuzzy_max_distance: 1,
            bias_enabled: true,
            bias_max_terms: 64,
        }
    }
}

/// Reject an entry with no replacement or an over-long one (Rule 12, §2). Cross-entry
/// dedupe is the command layer's job.
pub fn validate_entry(entry: &DictEntry) -> Result<(), String> {
    if entry.replacement.trim().is_empty() {
        return Err("entry must have a non-empty replacement".to_string());
    }
    if entry.replacement.chars().count() > MAX_REPLACEMENT_CHARS {
        return Err("replacement too long".to_string());
    }
    Ok(())
}

/// Every match key an entry contributes (its replacement + each soundsLike),
/// case-folded for case-insensitive entries (Rule 12 dedupe).
fn variant_keys(entry: &DictEntry) -> Vec<String> {
    std::iter::once(entry.replacement.clone())
        .chain(entry.sounds_like.clone())
        .map(|v| {
            let v = v.trim().to_string();
            if entry.case_sensitive {
                v
            } else {
                v.to_lowercase()
            }
        })
        .filter(|v| !v.is_empty())
        .collect()
}

/// The first variant of `candidate` that already exists in another entry, if any
/// (Rule 12). `exclude_id` skips the entry being updated. Pure → unit-tested.
pub fn duplicate_variant(entries: &[DictEntry], candidate: &DictEntry, exclude_id: &str) -> Option<String> {
    let new_keys = variant_keys(candidate);
    for existing in entries {
        if existing.id == exclude_id {
            continue;
        }
        let keys = variant_keys(existing);
        if let Some(hit) = new_keys.iter().find(|k| keys.contains(k)) {
            return Some(hit.clone());
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────
// Casing + fuzzy helpers (pure)
// ─────────────────────────────────────────────────────────────────────────────

/// Carry casing onto the replacement (Rules 2-4). Case-sensitive or a brand/mixed
/// replacement → verbatim; an all-lowercase replacement matched at a capitalized
/// token keeps the leading capital (preserves sentence case from cleanup).
pub fn match_case(matched: &str, replacement: &str, case_sensitive: bool) -> String {
    if case_sensitive || replacement.chars().any(|c| c.is_uppercase()) {
        return replacement.to_string();
    }
    if matched.chars().next().is_some_and(char::is_uppercase) {
        let mut chars = replacement.chars();
        if let Some(first) = chars.next() {
            return first.to_uppercase().collect::<String>() + chars.as_str();
        }
    }
    replacement.to_string()
}

/// Optimal string alignment (Damerau-Levenshtein with adjacent transpositions).
fn osa_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = best;
        }
    }
    d[n][m]
}

/// Bounded near-miss test (Rule 7-8): distance ≤ `max_distance`, and only for
/// variants longer than the short-word floor.
pub fn fuzzy_match(token: &str, variant: &str, max_distance: u8) -> bool {
    if variant.chars().count() <= FUZZY_MIN_LEN {
        return false;
    }
    osa_distance(&token.to_lowercase(), &variant.to_lowercase()) <= max_distance as usize
}

// ─────────────────────────────────────────────────────────────────────────────
// The matcher (pure)
// ─────────────────────────────────────────────────────────────────────────────

use crate::text_match::{reconstruct, tokenize, Tok};

/// A matchable variant (its words) bound to its source entry + scan order.
struct Variant<'a> {
    words: Vec<String>,
    entry: &'a DictEntry,
    order: usize,
}

fn build_variants(entries: &[DictEntry]) -> Vec<Variant<'_>> {
    let mut variants = Vec::new();
    for (order, entry) in entries.iter().enumerate() {
        if !entry.enabled || entry.replacement.trim().is_empty() {
            continue;
        }
        // Always match the canonical form (Rule 1 / idempotency) plus every soundsLike.
        let forms = std::iter::once(entry.replacement.clone()).chain(entry.sounds_like.clone());
        for form in forms {
            let words: Vec<String> = form.split_whitespace().map(str::to_string).collect();
            if !words.is_empty() {
                variants.push(Variant { words, entry, order });
            }
        }
    }
    variants
}

/// `Some(true)` exact, `Some(false)` fuzzy, `None` no match — for one token vs one
/// variant word, honoring case + fuzzy gating (Rules 2-3, 7-8).
fn word_eq(word: &str, vw: &str, entry: &DictEntry, settings: &DictSettings) -> Option<bool> {
    let exact = if entry.case_sensitive {
        word == vw
    } else {
        word.to_lowercase() == vw.to_lowercase()
    };
    if exact {
        return Some(true);
    }
    if settings.fuzzy_enabled_globally && entry.fuzzy && fuzzy_match(word, vw, settings.fuzzy_max_distance) {
        return Some(false);
    }
    None
}

type Candidate = (usize, String, bool, usize); // (word count, replacement, all-exact, order)

/// Longest span wins; ties break exact-over-fuzzy, then earlier entry (Rule 9).
fn pick_better(a: Candidate, b: Candidate) -> Candidate {
    if a.0 != b.0 {
        return if a.0 > b.0 { a } else { b };
    }
    if a.2 != b.2 {
        return if a.2 { a } else { b };
    }
    if a.3 <= b.3 {
        a
    } else {
        b
    }
}

fn best_match_at(
    words: &[&str],
    p: usize,
    variants: &[Variant],
    settings: &DictSettings,
) -> Option<Candidate> {
    let mut best: Option<Candidate> = None;
    for v in variants {
        let k = v.words.len();
        if p + k > words.len() {
            continue;
        }
        let mut all_exact = true;
        let mut matched = true;
        for off in 0..k {
            match word_eq(words[p + off], &v.words[off], v.entry, settings) {
                Some(true) => {}
                Some(false) => all_exact = false,
                None => {
                    matched = false;
                    break;
                }
            }
        }
        if !matched {
            continue;
        }
        let repl = match_case(words[p], &v.entry.replacement, v.entry.case_sensitive);
        let cand: Candidate = (k, repl, all_exact, v.order);
        best = Some(match best {
            None => cand,
            Some(prev) => pick_better(prev, cand),
        });
    }
    best
}

/// Enforce the dictionary on `text` (mechanism a, Rules 1-11). Pure + idempotent.
pub fn apply_dictionary(text: &str, entries: &[DictEntry], settings: &DictSettings) -> String {
    let variants = build_variants(entries);
    if variants.is_empty() {
        return text.to_string();
    }
    let toks = tokenize(text);
    let words: Vec<&str> = toks
        .iter()
        .filter_map(|t| match t {
            Tok::Word(w) => Some(w.as_str()),
            Tok::Sep(_) => None,
        })
        .collect();
    if words.is_empty() {
        return text.to_string();
    }

    // Plan replacements left-to-right; a placed match consumes its span (no cascading).
    let mut plan = crate::text_match::Plan::new();
    let mut p = 0;
    while p < words.len() {
        if let Some((k, repl, _, _)) = best_match_at(&words, p, &variants, settings) {
            plan.insert(p, (k, repl));
            p += k;
        } else {
            p += 1;
        }
    }
    if plan.is_empty() {
        return text.to_string();
    }
    // Reconstruct, dropping the interior separators of a multi-word match (shared).
    reconstruct(&toks, &plan)
}

/// Compose the Whisper initial-prompt bias string (mechanism b, Rule 13): the
/// enabled, `biasPrompt=true` entries' canonical forms, capped at `bias_max_terms`.
/// Empty when biasing is off.
pub fn build_bias_prompt(entries: &[DictEntry], settings: &DictSettings) -> String {
    if !settings.bias_enabled {
        return String::new();
    }
    entries
        .iter()
        .filter(|e| e.enabled && e.bias_prompt && !e.replacement.trim().is_empty())
        .take(settings.bias_max_terms as usize)
        .map(|e| e.replacement.clone())
        .collect::<Vec<_>>()
        .join(", ")
}

// ─────────────────────────────────────────────────────────────────────────────
// Persistence + managed state + commands (CRUD)
// ─────────────────────────────────────────────────────────────────────────────

/// On-disk shape: entries + global settings in one `dictionary.json` (§2 storage).
#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct DictFile {
    entries: Vec<DictEntry>,
    settings: DictSettings,
}

/// The in-memory dictionary (loaded once at startup). Lock order is always
/// settings-then-entries to avoid cross-command deadlock.
pub struct DictState {
    entries: Mutex<Vec<DictEntry>>,
    settings: Mutex<DictSettings>,
}

impl DictState {
    pub fn new(entries: Vec<DictEntry>, settings: DictSettings) -> Self {
        Self { entries: Mutex::new(entries), settings: Mutex::new(settings) }
    }

    fn settings_copy(&self) -> Result<DictSettings, String> {
        Ok(*self.settings.lock().map_err(|_| "dictionary state poisoned".to_string())?)
    }

    fn entries_copy(&self) -> Result<Vec<DictEntry>, String> {
        Ok(self.entries.lock().map_err(|_| "dictionary state poisoned".to_string())?.clone())
    }

    /// In-process snapshot for the orchestrator (dictation.rs).
    pub fn snapshot(&self) -> Result<(Vec<DictEntry>, DictSettings), String> {
        Ok((self.entries_copy()?, self.settings_copy()?))
    }
}

fn new_id() -> String {
    crate::persist::new_id("dict-")
}

fn dict_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("dictionary.json"))
}

/// Failure-safe load: missing or unparseable → empty dictionary + default settings.
pub fn load_dictionary(app: &AppHandle) -> (Vec<DictEntry>, DictSettings) {
    let Ok(path) = dict_path(app) else {
        return (Vec::new(), DictSettings::default());
    };
    let file: DictFile = crate::persist::load_json_or_default(&path);
    (file.entries, file.settings)
}

fn save_dictionary(app: &AppHandle, entries: &[DictEntry], settings: &DictSettings) -> Result<(), String> {
    let file = DictFile { entries: entries.to_vec(), settings: *settings };
    crate::persist::atomic_write_json(&dict_path(app)?, &file)
}

/// List all dictionary entries (Hub CRUD).
#[tauri::command]
pub fn dict_list(state: State<'_, DictState>) -> Result<Vec<DictEntry>, String> {
    state.entries_copy()
}

/// Validate, reject duplicate variants, assign an id, persist (Rule 12).
#[tauri::command]
pub fn dict_add(app: AppHandle, state: State<'_, DictState>, entry: DictEntry) -> Result<DictEntry, String> {
    validate_entry(&entry)?;
    let settings = state.settings_copy()?;
    let mut entries = state.entries.lock().map_err(|_| "dictionary state poisoned".to_string())?;
    if let Some(dup) = duplicate_variant(&entries, &entry, "") {
        return Err(format!("duplicate term: {dup}"));
    }
    let mut created = entry;
    created.id = new_id();
    entries.push(created.clone());
    save_dictionary(&app, &entries, &settings)?;
    Ok(created)
}

/// Update an entry by id (Rule 12 dedupe excludes itself).
#[tauri::command]
pub fn dict_update(app: AppHandle, state: State<'_, DictState>, entry: DictEntry) -> Result<DictEntry, String> {
    validate_entry(&entry)?;
    let settings = state.settings_copy()?;
    let mut entries = state.entries.lock().map_err(|_| "dictionary state poisoned".to_string())?;
    if !entries.iter().any(|e| e.id == entry.id) {
        return Err("entry not found".to_string());
    }
    if let Some(dup) = duplicate_variant(&entries, &entry, &entry.id) {
        return Err(format!("duplicate term: {dup}"));
    }
    for slot in entries.iter_mut() {
        if slot.id == entry.id {
            *slot = entry.clone();
        }
    }
    save_dictionary(&app, &entries, &settings)?;
    Ok(entry)
}

/// Remove an entry by id (idempotent).
#[tauri::command]
pub fn dict_remove(app: AppHandle, state: State<'_, DictState>, id: String) -> Result<(), String> {
    let settings = state.settings_copy()?;
    let mut entries = state.entries.lock().map_err(|_| "dictionary state poisoned".to_string())?;
    entries.retain(|e| e.id != id);
    save_dictionary(&app, &entries, &settings)
}

/// Read the global dictionary settings.
#[tauri::command]
pub fn dict_settings_get(state: State<'_, DictState>) -> Result<DictSettings, String> {
    state.settings_copy()
}

/// Replace the global dictionary settings and persist.
#[tauri::command]
pub fn dict_settings_set(
    app: AppHandle,
    state: State<'_, DictState>,
    settings: DictSettings,
) -> Result<DictSettings, String> {
    {
        let mut slot = state.settings.lock().map_err(|_| "dictionary state poisoned".to_string())?;
        *slot = settings;
    }
    let entries = state.entries_copy()?;
    save_dictionary(&app, &entries, &settings)?;
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(replacement: &str, sounds_like: &[&str]) -> DictEntry {
        DictEntry {
            replacement: replacement.to_string(),
            sounds_like: sounds_like.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    fn apply(text: &str, entries: &[DictEntry]) -> String {
        apply_dictionary(text, entries, &DictSettings::default())
    }

    #[test]
    fn exact_variant_replaced() {
        let e = vec![entry("MIA", &["mia"])];
        assert_eq!(apply("open mia now", &e), "open MIA now");
    }

    #[test]
    fn case_insensitive_outputs_verbatim() {
        let e = vec![entry("React", &[])];
        assert_eq!(apply("REACT and react and React", &e), "React and React and React");
    }

    #[test]
    fn case_sensitive_only_exact_case() {
        let mut it = entry("IT", &[]);
        it.case_sensitive = true;
        let e = vec![it];
        // lowercase "it" must NOT match; exact "IT" stays "IT".
        assert_eq!(apply("the it dept vs IT", &e), "the it dept vs IT");
    }

    #[test]
    fn whole_word_no_partial_hits() {
        let e = vec![entry("CAT", &["cat"])];
        assert_eq!(apply("category and a cat", &e), "category and a CAT");
    }

    #[test]
    fn multi_word_phrase_collapses() {
        let e = vec![entry("React", &["react js"])];
        assert_eq!(apply("i use react js daily", &e), "i use React daily");
    }

    #[test]
    fn fuzzy_gated_and_default_off_per_entry() {
        let mut react = entry("React", &["react"]);
        react.fuzzy = true;
        // "raect" is a 1-distance transposition of "react".
        assert_eq!(apply("i like raect", &[react.clone()]), "i like React");
        // Without per-entry fuzzy, no near-miss correction.
        let plain = entry("React", &["react"]);
        assert_eq!(apply("i like raect", &[plain]), "i like raect");
    }

    #[test]
    fn fuzzy_skips_short_variants() {
        let mut e = entry("CAT", &["cat"]);
        e.fuzzy = true;
        // "bat" is distance 1 from "cat" but "cat" is at the short-word floor → no fuzzy.
        assert_eq!(apply("a bat flew", &[e]), "a bat flew");
    }

    #[test]
    fn longest_match_wins_on_overlap() {
        let short = entry("Reactt", &["react"]);
        let long = entry("ReactJS", &["react js"]);
        // "react js" should take the 2-word entry, not the 1-word one.
        assert_eq!(apply("use react js here", &[short, long]), "use ReactJS here");
    }

    #[test]
    fn idempotent() {
        let e = vec![entry("MIA", &["mia"])];
        let once = apply("mia and mia", &e);
        assert_eq!(apply(&once, &e), once);
    }

    #[test]
    fn disabled_and_empty_are_inert() {
        let mut disabled = entry("MIA", &["mia"]);
        disabled.enabled = false;
        assert_eq!(apply("open mia now", &[disabled]), "open mia now");
        assert_eq!(apply("open mia now", &[]), "open mia now");
    }

    #[test]
    fn match_case_carries_sentence_capital() {
        // all-lowercase replacement, capitalized match at sentence start → leading cap.
        assert_eq!(match_case("Okay", "ok", false), "Ok");
        assert_eq!(match_case("okay", "ok", false), "ok");
        // brand replacement is always verbatim.
        assert_eq!(match_case("mia", "MIA", false), "MIA");
        assert_eq!(match_case("It", "it", true), "it"); // case-sensitive → verbatim
    }

    #[test]
    fn osa_distance_basics() {
        assert_eq!(osa_distance("react", "react"), 0);
        assert_eq!(osa_distance("react", "raect"), 1); // transposition
        assert_eq!(osa_distance("react", "reactt"), 1); // insertion
        assert_eq!(osa_distance("react", "rect"), 1); // deletion
    }

    #[test]
    fn bias_prompt_lists_enabled_terms() {
        let mut off = entry("Hidden", &[]);
        off.bias_prompt = false;
        let e = vec![entry("MIA", &[]), entry("React", &[]), off];
        assert_eq!(build_bias_prompt(&e, &DictSettings::default()), "MIA, React");
        let disabled = DictSettings { bias_enabled: false, ..Default::default() };
        assert_eq!(build_bias_prompt(&e, &disabled), "");
    }

    #[test]
    fn validate_entry_rejects_empty_and_long() {
        assert!(validate_entry(&entry("MIA", &[])).is_ok());
        assert_eq!(
            validate_entry(&entry("   ", &[])),
            Err("entry must have a non-empty replacement".to_string())
        );
        let long = entry(&"x".repeat(201), &[]);
        assert_eq!(validate_entry(&long), Err("replacement too long".to_string()));
    }

    #[test]
    fn duplicate_variant_detects_collisions() {
        let mut a = entry("MIA", &["mia"]);
        a.id = "a".to_string();
        let existing = vec![a];
        // New entry whose soundsLike collides (case-folded) with an existing variant.
        let dup = entry("Mya", &["MIA"]);
        assert_eq!(duplicate_variant(&existing, &dup, ""), Some("mia".to_string()));
        // A fresh entry with no overlap is fine.
        assert_eq!(duplicate_variant(&existing, &entry("React", &["react"]), ""), None);
        // Updating the same entry (excluded by id) does not collide with itself.
        let mut self_update = entry("MIA", &["mia"]);
        self_update.id = "a".to_string();
        assert_eq!(duplicate_variant(&existing, &self_update, "a"), None);
    }
}
