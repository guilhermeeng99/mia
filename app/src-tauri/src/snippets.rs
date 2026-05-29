//! Snippets — voice-triggered text expansion (ADR-008, Phase 3). A spoken trigger
//! ("minha assinatura") expands to longer canned text. Pure, deterministic string
//! work in the text stage, **after** cleanup + the custom dictionary, **before**
//! injection. See `docs/specs/snippets.md`.
//!
//! This file is the **pure, cargo-tested core**: `compile_snippets`,
//! `normalize_trigger` (case- + accent-fold), `expand_snippets` (whole-phrase,
//! word-boundary, longest-first, no recursion), `apply_case`, `validate_snippet`.
//! The CRUD commands + `snippets.json` persistence + managed state are the
//! follow-up (mirroring the dictionary module).

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use unicode_normalization::UnicodeNormalization;

/// Where a trigger may match within an utterance (§2). Default `Anywhere`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SnippetAnchor {
    #[default]
    Anywhere,
    StartOnly,
}

/// Optional case transform on insert (§2). Default `Verbatim`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SnippetCase {
    #[default]
    Verbatim,
    MatchSentence,
}

/// A user-defined expansion (§2). Defaults match §4.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Snippet {
    pub id: String,
    pub trigger: String,
    pub expansion: String,
    pub anchor: SnippetAnchor,
    pub case: SnippetCase,
    pub enabled: bool,
}

impl Default for Snippet {
    fn default() -> Self {
        Self {
            id: String::new(),
            trigger: String::new(),
            expansion: String::new(),
            anchor: SnippetAnchor::Anywhere,
            case: SnippetCase::Verbatim,
            enabled: true,
        }
    }
}

/// The result of expanding an utterance — the new text + which triggers fired (§2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpansionResult {
    pub output: String,
    pub applied_triggers: Vec<String>,
}

/// Reject an empty trigger/expansion (§2). Duplicate-trigger conflict is
/// command-level.
pub fn validate_snippet(snippet: &Snippet) -> Result<(), String> {
    if snippet.trigger.trim().is_empty() {
        return Err("trigger cannot be empty".to_string());
    }
    if snippet.expansion.trim().is_empty() {
        return Err("expansion cannot be empty".to_string());
    }
    Ok(())
}

/// True if another snippet already uses `candidate`'s normalized trigger (Rule 13).
/// `exclude_id` skips the snippet being updated. Pure → unit-tested.
pub fn duplicate_trigger(snippets: &[Snippet], candidate: &Snippet, exclude_id: &str) -> bool {
    let key = normalize_trigger(&candidate.trigger);
    snippets
        .iter()
        .any(|s| s.id != exclude_id && normalize_trigger(&s.trigger) == key)
}

// ─────────────────────────────────────────────────────────────────────────────
// Folding + tokenizing (pure)
// ─────────────────────────────────────────────────────────────────────────────

/// Fold one token to its match key: NFD-decompose, drop combining diacritical
/// marks (so pt-BR `endereço` ≈ `endereco`, `é` ≈ `e`), then lowercase (Rule 3).
fn fold(token: &str) -> String {
    token
        .nfd()
        .filter(|c| !('\u{0300}'..='\u{036F}').contains(c))
        .collect::<String>()
        .to_lowercase()
}

/// The comparison key for a trigger phrase: fold + collapse internal whitespace.
pub fn normalize_trigger(trigger: &str) -> String {
    fold(trigger).split_whitespace().collect::<Vec<_>>().join(" ")
}

enum Tok {
    Word(String),
    Sep(String),
}

fn tokenize(text: &str) -> Vec<Tok> {
    let mut toks = Vec::new();
    let mut cur = String::new();
    let mut cur_word = false;
    for ch in text.chars() {
        let is_word = ch.is_alphanumeric();
        if cur.is_empty() {
            cur_word = is_word;
        } else if is_word != cur_word {
            toks.push(if cur_word { Tok::Word(cur.clone()) } else { Tok::Sep(cur.clone()) });
            cur.clear();
            cur_word = is_word;
        }
        cur.push(ch);
    }
    if !cur.is_empty() {
        toks.push(if cur_word { Tok::Word(cur) } else { Tok::Sep(cur) });
    }
    toks
}

// ─────────────────────────────────────────────────────────────────────────────
// Compiled set + expansion (pure)
// ─────────────────────────────────────────────────────────────────────────────

struct Compiled {
    words: Vec<String>, // folded trigger words
    trigger: String,    // original, for reporting applied triggers
    expansion: String,
    anchor: SnippetAnchor,
    case: SnippetCase,
}

/// The in-memory snippet set, longest-trigger-first so the most specific wins.
pub struct SnippetSet {
    compiled: Vec<Compiled>,
}

/// Filter enabled snippets, fold triggers, sort longest-trigger-first (Rule 5/9).
pub fn compile_snippets(snippets: &[Snippet]) -> SnippetSet {
    let mut compiled: Vec<Compiled> = snippets
        .iter()
        .filter(|s| s.enabled && !s.trigger.trim().is_empty() && !s.expansion.trim().is_empty())
        .map(|s| {
            let key = normalize_trigger(&s.trigger);
            Compiled {
                words: key.split(' ').filter(|w| !w.is_empty()).map(str::to_string).collect(),
                trigger: s.trigger.clone(),
                expansion: s.expansion.clone(),
                anchor: s.anchor,
                case: s.case,
            }
        })
        .filter(|c| !c.words.is_empty())
        .collect();
    // Stable sort by trigger word-count desc — original order breaks ties.
    compiled.sort_by_key(|c| std::cmp::Reverse(c.words.len()));
    SnippetSet { compiled }
}

/// Apply the per-snippet case transform (Rule 6/§2). `at_start` is true when the
/// match begins the utterance (drives `MatchSentence`).
fn apply_case(expansion: &str, case: SnippetCase, at_start: bool) -> String {
    if case == SnippetCase::Verbatim || !at_start {
        return expansion.to_string();
    }
    let mut chars = expansion.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Expand snippet triggers in `text` (the pure core, Rules 1-9). Whole-phrase,
/// word-boundary, longest-first; the expansion is inserted verbatim and **not**
/// re-scanned (no nesting). Returns the new text + the triggers that fired.
pub fn expand_snippets(text: &str, set: &SnippetSet) -> ExpansionResult {
    if set.compiled.is_empty() {
        return ExpansionResult { output: text.to_string(), applied_triggers: Vec::new() };
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
        return ExpansionResult { output: text.to_string(), applied_triggers: Vec::new() };
    }
    let folded: Vec<String> = words.iter().map(|w| fold(w)).collect();

    // Plan replacements left-to-right; a placed match consumes its span (no recursion).
    let mut plan: std::collections::HashMap<usize, (usize, String)> = std::collections::HashMap::new();
    let mut applied = Vec::new();
    let mut p = 0;
    while p < words.len() {
        if let Some(c) = find_match(&folded, p, set) {
            let k = c.words.len();
            plan.insert(p, (k, apply_case(&c.expansion, c.case, p == 0)));
            applied.push(c.trigger.clone());
            p += k;
        } else {
            p += 1;
        }
    }
    if plan.is_empty() {
        return ExpansionResult { output: text.to_string(), applied_triggers: applied };
    }

    let output = reconstruct(&toks, &plan);
    ExpansionResult { output, applied_triggers: applied }
}

/// The first (longest-first) snippet whose folded trigger matches at word `p`,
/// honoring its anchor (Rules 4, 5).
fn find_match<'a>(folded: &[String], p: usize, set: &'a SnippetSet) -> Option<&'a Compiled> {
    set.compiled.iter().find(|c| {
        if c.anchor == SnippetAnchor::StartOnly && p != 0 {
            return false;
        }
        let k = c.words.len();
        p + k <= folded.len() && folded[p..p + k] == c.words[..]
    })
}

/// Rebuild the string from the token stream, applying the plan and dropping the
/// interior separators of a multi-word trigger.
fn reconstruct(toks: &[Tok], plan: &std::collections::HashMap<usize, (usize, String)>) -> String {
    let mut out = String::new();
    let mut wpos = 0usize;
    let mut i = 0usize;
    while i < toks.len() {
        match &toks[i] {
            Tok::Sep(s) => {
                out.push_str(s);
                i += 1;
            }
            Tok::Word(w) => {
                if let Some((k, repl)) = plan.get(&wpos) {
                    out.push_str(repl);
                    let mut consumed = 0;
                    while i < toks.len() && consumed < *k {
                        if matches!(toks[i], Tok::Word(_)) {
                            consumed += 1;
                        }
                        i += 1;
                        if consumed == *k {
                            break;
                        }
                    }
                    wpos += k;
                } else {
                    out.push_str(w);
                    i += 1;
                    wpos += 1;
                }
            }
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Persistence + managed state + commands (CRUD)
// ─────────────────────────────────────────────────────────────────────────────

/// The in-memory snippet list (loaded once at startup), persisted to `snippets.json`.
pub struct SnippetState {
    inner: Mutex<Vec<Snippet>>,
}

impl SnippetState {
    pub fn new(snippets: Vec<Snippet>) -> Self {
        Self { inner: Mutex::new(snippets) }
    }

    fn list(&self) -> Result<Vec<Snippet>, String> {
        Ok(self.inner.lock().map_err(|_| "snippet state poisoned".to_string())?.clone())
    }

    /// In-process snapshot for the orchestrator (dictation.rs).
    pub fn snapshot(&self) -> Result<Vec<Snippet>, String> {
        self.list()
    }
}

fn new_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("snip-{nanos}")
}

fn snippets_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("snippets.json"))
}

/// Failure-safe load: missing or unparseable → empty set (never a startup error).
pub fn load_snippets(app: &AppHandle) -> Vec<Snippet> {
    let Ok(path) = snippets_path(app) else {
        return Vec::new();
    };
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_snippets(app: &AppHandle, snippets: &[Snippet]) -> Result<(), String> {
    let path = snippets_path(app)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(snippets).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| format!("failed to write snippets file: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("failed to write snippets file: {e}"))
}

/// List all snippets (Hub CRUD).
#[tauri::command]
pub fn list_snippets(state: State<'_, SnippetState>) -> Result<Vec<Snippet>, String> {
    state.list()
}

/// Create (empty id) or update (existing id) a snippet — validates + rejects a
/// duplicate normalized trigger (Rule 12-13).
#[tauri::command]
pub fn upsert_snippet(
    app: AppHandle,
    state: State<'_, SnippetState>,
    snippet: Snippet,
) -> Result<Snippet, String> {
    validate_snippet(&snippet)?;
    let mut list = state.inner.lock().map_err(|_| "snippet state poisoned".to_string())?;
    if duplicate_trigger(&list, &snippet, &snippet.id) {
        return Err("a snippet with this trigger already exists".to_string());
    }
    let mut saved = snippet;
    if saved.id.trim().is_empty() {
        saved.id = new_id();
        list.push(saved.clone());
    } else if let Some(slot) = list.iter_mut().find(|s| s.id == saved.id) {
        *slot = saved.clone();
    } else {
        return Err("snippet not found".to_string());
    }
    save_snippets(&app, &list)?;
    Ok(saved)
}

/// Delete a snippet by id (idempotent).
#[tauri::command]
pub fn delete_snippet(app: AppHandle, state: State<'_, SnippetState>, id: String) -> Result<(), String> {
    let mut list = state.inner.lock().map_err(|_| "snippet state poisoned".to_string())?;
    list.retain(|s| s.id != id);
    save_snippets(&app, &list)
}

/// Preview what the current snippets do to a sample utterance (Hub, no dictation).
#[tauri::command]
pub fn preview_expansion(state: State<'_, SnippetState>, text: String) -> Result<ExpansionResult, String> {
    let list = state.list()?;
    Ok(expand_snippets(&text, &compile_snippets(&list)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snip(trigger: &str, expansion: &str) -> Snippet {
        Snippet { trigger: trigger.to_string(), expansion: expansion.to_string(), ..Default::default() }
    }

    fn expand(text: &str, snippets: &[Snippet]) -> ExpansionResult {
        expand_snippets(text, &compile_snippets(snippets))
    }

    #[test]
    fn expands_a_phrase_trigger() {
        let s = vec![snip("minha assinatura", "João Silva\nCEO")];
        let r = expand("segue minha assinatura obrigado", &s);
        assert_eq!(r.output, "segue João Silva\nCEO obrigado");
        assert_eq!(r.applied_triggers, vec!["minha assinatura"]);
    }

    #[test]
    fn matching_is_case_and_accent_insensitive() {
        let s = vec![snip("endereço comercial", "Rua X, 100")];
        // No accent + different case in the utterance still matches.
        let r = expand("meu Endereco Comercial aqui", &s);
        assert_eq!(r.output, "meu Rua X, 100 aqui");
    }

    #[test]
    fn no_substring_match_inside_a_word() {
        let s = vec![snip("sig", "SIGNATURE")];
        assert_eq!(expand("my signature here", &s).output, "my signature here");
    }

    #[test]
    fn longest_trigger_wins() {
        let s = vec![snip("minha", "X"), snip("minha assinatura", "FULL")];
        assert_eq!(expand("minha assinatura", &s).output, "FULL");
    }

    #[test]
    fn start_only_anchor() {
        let mut s = snip("ola", "Olá!");
        s.anchor = SnippetAnchor::StartOnly;
        assert_eq!(expand("ola mundo", &[s.clone()]).output, "Olá! mundo");
        assert_eq!(expand("digo ola agora", &[s]).output, "digo ola agora"); // not at start
    }

    #[test]
    fn expansion_is_verbatim_and_not_re_expanded() {
        // Expansion contains another trigger's text but must NOT be re-expanded (Rule 7).
        let s = vec![snip("a", "minha assinatura"), snip("minha assinatura", "LOOP")];
        let r = expand("a", &s);
        assert_eq!(r.output, "minha assinatura");
        assert_eq!(r.applied_triggers, vec!["a"]);
    }

    #[test]
    fn disabled_and_empty_are_inert() {
        let mut disabled = snip("ola", "Olá");
        disabled.enabled = false;
        assert_eq!(expand("ola", &[disabled]).output, "ola");
        assert_eq!(expand("ola", &[]).output, "ola");
    }

    #[test]
    fn match_sentence_capitalizes_at_start() {
        let mut s = snip("ola", "olá pessoal");
        s.case = SnippetCase::MatchSentence;
        assert_eq!(expand("ola", &[s.clone()]).output, "Olá pessoal"); // at start
        assert_eq!(expand("eu digo ola", &[s]).output, "eu digo olá pessoal"); // not start → verbatim
    }

    #[test]
    fn normalize_trigger_folds_case_accent_whitespace() {
        assert_eq!(normalize_trigger("  Minha   Assinatura "), "minha assinatura");
        assert_eq!(normalize_trigger("Endereço"), "endereco");
    }

    #[test]
    fn validate_rejects_empty() {
        assert!(validate_snippet(&snip("ola", "Olá")).is_ok());
        assert_eq!(validate_snippet(&snip("  ", "x")), Err("trigger cannot be empty".to_string()));
        assert_eq!(validate_snippet(&snip("x", "  ")), Err("expansion cannot be empty".to_string()));
    }

    #[test]
    fn duplicate_trigger_detects_normalized_collisions() {
        let mut a = snip("minha assinatura", "X");
        a.id = "a".to_string();
        let existing = vec![a];
        // Same trigger, different case/spacing → collision.
        assert!(duplicate_trigger(&existing, &snip("Minha  Assinatura", "Y"), ""));
        // Different trigger → fine.
        assert!(!duplicate_trigger(&existing, &snip("outro", "Y"), ""));
        // Updating itself (excluded by id) → not a collision.
        let mut self_update = snip("minha assinatura", "Z");
        self_update.id = "a".to_string();
        assert!(!duplicate_trigger(&existing, &self_update, "a"));
    }
}
