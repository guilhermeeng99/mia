# Custom Dictionary (Personal Vocabulary) Feature Spec

> **Status**: Phase 3 — pure mechanism-(a) core implemented & cargo-tested in `dictionary.rs`: `apply_dictionary` (exact / case / whole-word / multi-word / fuzzy / longest-match / idempotent), `match_case`, `osa_distance`+`fuzzy_match`, `build_bias_prompt`, `validate_entry` (Rules 1-13). Runtime-pending: the CRUD commands + `dictionary.json` persistence + managed state, cross-entry duplicate rejection, and wiring the bias prompt into the warm-Whisper call.
> **Last updated**: 2026-05-29
> **Coverage**: Sections 1-9 drafted (engine contract + business rules are the load-bearing parts)
> **Environment**: desktop (Windows, native)

The **custom dictionary** is a user-managed list of personal vocabulary — names, jargon,
acronyms, brand/product names, and domain terms — that MIA enforces in dictation output so they
come out spelled and cased the way the user wants (e.g. `mia` → `MIA`, `tcheka` → `Tchéka`,
`react js` → `React`). It sits in the **text stage** of the dictation pipeline
(hotkey → capture → VAD → STT → **cleanup/dictionary** → inject), right alongside the
deterministic cleanup module. MIA offers **two cooperating mechanisms**: (a) **post-transcription
replacement** — deterministic find/replace plus fuzzy matching of near-misses, applied during or
just after [text-cleanup.md](text-cleanup.md) — which is the reliable Phase-3 **baseline**; and
(b) **Whisper initial-prompt biasing** — feeding the user's terms as the model's initial prompt
to nudge recognition toward them ([speech-to-text.md](speech-to-text.md)). It lands in **Phase 3
— Personalization** (see [../ROADMAP.md](../ROADMAP.md)) and implements the hybrid text-intelligence
direction of [ADR-008](architecture.md#adr-008-hybrid-text-intelligence--deterministic-cleanup-phase-1--optional-local-llm-phase-2):
fidelity-safe, deterministic-first, model help on top — never the LLM.

**Scope decisions** (locked at design time):

- **Replacement (mechanism a) is the baseline; biasing (mechanism b) is an assist.** Find/replace
  is deterministic, testable, and works on every engine/model. Initial-prompt biasing is
  best-effort and model-dependent, so it never replaces (a) — it only reduces how many corrections
  (a) has to make. If biasing is disabled or unavailable, dictionary enforcement still fully works
  via (a) (ADR-008 / Phase 3).
- **Deterministic / rule-based, not LLM.** Dictionary enforcement is a pure Rust module
  (exact + fuzzy string matching), with **no** local LLM involvement. The optional LLM
  ([ai-commands.md](ai-commands.md)) is a separate Phase-2 feature and is never on the dictionary
  hot path — keeps the default path faithful and fast (ADR-008).
- **Fuzzy matching is opt-in per entry and conservative.** Auto-correcting "near-misses" risks
  clobbering legitimate words. Fuzzy matching is off unless the entry declares `soundsLike`
  variants and/or enables fuzzy, and it is bounded by an edit-distance threshold and a word-boundary
  rule (Phase 3).
- **Lives with cleanup, after STT.** The dictionary rewrite runs on the cleaned transcript string,
  in the same pure text stage as [text-cleanup.md](text-cleanup.md), before injection — so it is
  language-agnostic at the string level and engine-independent.
- **Local storage, no cloud.** The dictionary is a plain JSON file in app-data; it never syncs or
  leaves the machine ([ADR-001](architecture.md#adr-001-native-on-device-privacy-first)).
- **Auto-learn from corrections is deferred.** Inferring new dictionary entries from how the user
  edits MIA's output is a later enhancement, not Phase 3 (see [§9](#9-out-of-scope-this-version)).

---

## 1. Inputs / Outputs

This feature has two faces: a **text-stage rewrite** in the live pipeline, and a **CRUD surface**
in the Hub. It captures no audio of its own.

| Aspect | This feature |
|---|---|
| **Trigger** | Live: every dictated utterance (rewrite runs automatically in the text stage). Management: user edits the dictionary in the Settings/Hub window. |
| **Audio in** | N/A — operates on text only (no mic). |
| **Text in** | The **cleaned transcript** string from [text-cleanup.md](text-cleanup.md) (mechanism a). For mechanism b, the user's term list is composed into Whisper's **initial prompt** *before* STT. |
| **Text out** | The same string with dictionary terms enforced (correct spelling/casing), handed onward to injection ([text-injection.md](text-injection.md)). Management path: the dictionary JSON persisted to app-data. |
| **Target** | The OS-focused window (via the normal inject path); the Settings/Hub window for CRUD. |
| **Language** | Language-agnostic at the string level; pt-BR and English are first-class for `soundsLike` examples. The biasing prompt is fed in the dictation language. |

Backing crates/modules: the rewrite is **pure Rust** in `app/src-tauri/src/dictionary.rs` (no
I/O, no model) called from the same text stage as `cleanup.rs`. Fuzzy matching uses a small
permissive edit-distance helper (e.g. `strsim`, MIT/Apache-2.0 — [ADR-010](architecture.md#adr-010-licensing--mit-app-permissive-deps-only)).
Initial-prompt biasing composes a string passed to the warm Whisper model
([speech-to-text.md](speech-to-text.md)). The dictionary buffer is a small in-memory `Vec` of
entries; **no audio touches disk** (ADR-001) — only the JSON dictionary is persisted.

---

## 2. Engine Contract (Rust)

Rust is the **engine**; the Svelte UI is a thin webview that calls typed `invoke()` wrappers (see
[architecture.md](architecture.md)). All commands return `Result<T, String>`
([ADR-006](architecture.md#adr-006-resultt-string-error-model-across-the-rust--ui-ipc)).

**Module**: `app/src-tauri/src/dictionary.rs`

```rust
// ---- Data model ----------------------------------------------------------
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DictEntry {
    pub id: String,                 // stable uuid (generated on add)
    pub replacement: String,        // canonical output form, e.g. "MIA", "Tchéka"
    pub sounds_like: Vec<String>,   // optional surface variants STT may emit, e.g. ["mia","m i a","mya"]
    pub case_sensitive: bool,       // default false: match regardless of case, output `replacement` verbatim
    pub whole_word: bool,           // default true: match only on word boundaries (no partial-word hits)
    pub fuzzy: bool,                // default false: allow bounded edit-distance near-miss matching
    pub bias_prompt: bool,          // default true: include this term in the Whisper initial prompt (mech. b)
    pub enabled: bool,              // default true
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DictSettings {
    pub fuzzy_enabled_globally: bool, // master switch for mechanism-a fuzzy (default true; per-entry still gated)
    pub fuzzy_max_distance: u8,       // Damerau-Levenshtein cap for fuzzy hits (default 1)
    pub bias_enabled: bool,           // master switch for mechanism b (default true)
    pub bias_max_terms: u16,          // cap how many terms go into the initial prompt (default 64)
}

// ---- Commands (CRUD; the UI holds no rewrite logic) ----------------------
#[tauri::command]
async fn dict_list() -> Result<Vec<DictEntry>, String>;
#[tauri::command]
async fn dict_add(entry: DictEntry) -> Result<DictEntry, String>;     // validates, assigns id, persists
#[tauri::command]
async fn dict_update(entry: DictEntry) -> Result<DictEntry, String>;  // by id
#[tauri::command]
async fn dict_remove(id: String) -> Result<(), String>;
#[tauri::command]
async fn dict_settings_get() -> Result<DictSettings, String>;
#[tauri::command]
async fn dict_settings_set(settings: DictSettings) -> Result<DictSettings, String>;
#[tauri::command]
async fn dict_import(json: String) -> Result<usize, String>;          // bulk add/merge; returns count
#[tauri::command]
async fn dict_export() -> Result<String, String>;                     // dictionary JSON for backup/sharing

// ---- Pure helpers (#[cfg(test)], no I/O) — the testable core -------------
// apply_dictionary(text, &[DictEntry], &DictSettings) -> String   // mechanism a; pure
// fuzzy_match(token, variant, max_distance) -> bool               // bounded Damerau-Levenshtein
// match_case(matched: &str, replacement: &str, case_sensitive: bool) -> String  // casing carry-over
// build_bias_prompt(&[DictEntry], &DictSettings, lang) -> String  // mechanism b prompt composer
// validate_entry(&DictEntry) -> Result<(), String>               // dedupe/empty/length checks
```

- **`DictEntry` fields** — `replacement` (required, the canonical output), `soundsLike` (optional
  surface variants to match on; if empty, the matcher derives a default variant from `replacement`),
  `caseSensitive` (default `false`), `wholeWord` (default `true`), `fuzzy` (default `false`),
  `biasPrompt` (default `true`), `enabled` (default `true`).
- **`Err(String)` cases**: `"entry must have a non-empty replacement"`,
  `"duplicate term: <variant>"`, `"replacement too long"` (defensive length cap),
  `"invalid dictionary json: <detail>"` (import), `"dictionary file write failed: <detail>"`.
  Each maps 1:1 to a Hub validation/error state.
- **No native model, no sidecar**: mechanism (a) is pure Rust; mechanism (b) only contributes a
  string to the existing warm-Whisper call. This feature cold-spawns nothing.
- **Pure helpers** above (`apply_dictionary`, `fuzzy_match`, `match_case`, `build_bias_prompt`,
  `validate_entry`) take no I/O and carry `#[cfg(test)]` cargo tests — the rewrite logic is the
  load-bearing, fully testable core.
- **Typed UI wrapper**: `app/src/lib/dictionary.ts` (`invoke<DictEntry[]>("dict_list")`, etc.),
  one wrapper per command group; the UI holds **no** rewrite logic (see
  [settings.md](settings.md)).
- **Storage**: a single `dictionary.json` in the app-data dir (alongside settings), loaded once
  into managed `State` and rewritten atomically on each mutation (write `.tmp` → rename, mirroring
  the `.part` → final-name discipline of [ADR-007](architecture.md#adr-007-on-demand-model-download--cpu-bundled--optional-cuda-engine)).

---

## 3. Business Rules

Numbered, testable, unambiguous. The mechanism-(a) rules all exercise the pure
`apply_dictionary` / `fuzzy_match` / `match_case` helpers → `cargo test`.

1. **Exact variant match wins.** If a token (or token sequence) in the input equals an entry's
   `replacement` or any of its `soundsLike` variants (per the entry's case rule), it is replaced
   with `replacement`. Example: entry `{replacement:"MIA", soundsLike:["mia"]}` turns
   `"open mia now"` → `"open MIA now"`.

2. **Case-insensitive by default.** With `caseSensitive=false`, matching ignores case and the
   output is `replacement` **verbatim** (the canonical casing always wins). `"REACT"`, `"react"`,
   and `"React"` all become the configured `replacement` `"React"`.

3. **Case-sensitive when requested.** With `caseSensitive=true`, only exact-case occurrences match.
   This protects terms whose lowercase form is a common word (e.g. only `"IT"` the department, not
   the pronoun `"it"`).

4. **Sentence-case carry-over for derived variants.** When `caseSensitive=false` and the matched
   token is a capitalized form at a sentence start while `replacement` is lowercase, `match_case`
   preserves the leading-capital of the matched token unless `replacement` is explicitly
   mixed/upper-cased (a brand like `"iPhone"`/`"MIA"` always outputs verbatim). This keeps
   sentence capitalization from cleanup intact ([text-cleanup.md](text-cleanup.md)).

5. **Whole-word matching by default — no partial-word hits.** With `wholeWord=true` (default), a
   term matches only at word boundaries; `"cat"` never rewrites inside `"category"`. Boundaries are
   Unicode-aware so pt-BR accented letters count as word characters.

6. **Multi-word variants match as a phrase.** A `soundsLike` variant containing spaces (e.g.
   `"react js"`, `"m i a"`) matches the corresponding consecutive token sequence and is replaced as
   one unit by `replacement`.

7. **Fuzzy matching is bounded and gated.** Fuzzy matching applies **only** when both
   `fuzzy_enabled_globally` and the entry's `fuzzy=true`. A token matches a variant when its
   Damerau-Levenshtein distance ≤ `fuzzy_max_distance` (default `1`) **and** it satisfies the
   `wholeWord` boundary rule. Exact matches (Rule 1) always take priority over fuzzy candidates.

8. **Fuzzy never fires on very short tokens.** To avoid wrecking small common words, fuzzy
   matching is skipped when the variant length ≤ a floor (e.g. ≤ 3 chars) regardless of distance —
   short terms must match exactly.

9. **Longest-match / most-specific entry wins on overlap.** When two entries could match
   overlapping spans, the entry whose matched span is **longest** wins; ties break by exact-over-fuzzy,
   then by entry order. The matcher scans left-to-right and does not re-match inside an
   already-replaced span (no cascading rewrites).

10. **Idempotent.** Running `apply_dictionary` on its own output yields the same string — replacing
    a token with its canonical `replacement` must not re-trigger another entry into an infinite or
    drifting rewrite.

11. **Disabled and empty entries are inert.** `enabled=false` entries are skipped entirely. An
    empty dictionary (or all-disabled) is a no-op that returns the input unchanged.

12. **Validation on write.** `dict_add`/`dict_update` reject an empty `replacement`, a variant that
    duplicates another entry's variant (case-folded for case-insensitive entries), and
    over-length values — returning the matching `Err(String)` from [§2](#2-engine-contract-rust).

13. **Biasing (mechanism b) is recognition-only and best-effort.** `build_bias_prompt` composes up
    to `bias_max_terms` of the enabled, `biasPrompt=true` entries' `replacement` forms into Whisper's
    initial prompt for the utterance. It **changes recognition probability, not guarantees**;
    whatever the model emits is still passed through mechanism (a). Biasing never alters the text
    directly and is fully disabled when `bias_enabled=false`.

14. **Order in the pipeline.** Dictionary rewrite (mechanism a) runs **after** spoken-punctuation
    substitution and filler removal but as part of the same text stage, so it sees normalized
    whitespace and casing from [text-cleanup.md](text-cleanup.md); its output is the string handed to
    injection.

---

## 4. Options & Defaults

Per-entry options live on `DictEntry`; global toggles live on `DictSettings`. Anti-hallucination
STT defaults remain fixed and are unaffected by this feature ([ADR-007](architecture.md#adr-007-on-demand-model-download--cpu-bundled--optional-cuda-engine));
biasing only contributes the *initial prompt* string, not the decoding flags.

| Option | Type | Range / values | Default | Effect |
|---|---|---|---|---|
| `replacement` (entry) | string | non-empty, length-capped | — (required) | The canonical output form emitted on a match. |
| `soundsLike` (entry) | string[] | 0..N surface variants | `[]` | Extra forms STT may emit; if empty, a default variant is derived from `replacement`. |
| `caseSensitive` (entry) | bool | — | `false` | `false`: match any case, output `replacement` verbatim. `true`: match exact case only. |
| `wholeWord` (entry) | bool | — | `true` | `true`: word-boundary match only (no partial-word hits). |
| `fuzzy` (entry) | bool | — | `false` | Allow bounded near-miss matching (gated by the global switch + length floor). |
| `biasPrompt` (entry) | bool | — | `true` | Include this term in the Whisper initial prompt (mechanism b). |
| `enabled` (entry) | bool | — | `true` | Disabled entries are skipped by both mechanisms. |
| `fuzzyEnabledGlobally` | bool | — | `true` | Master switch for mechanism-a fuzzy matching. |
| `fuzzyMaxDistance` | int | `1..2` | `1` | Damerau-Levenshtein cap for a fuzzy hit. |
| `biasEnabled` | bool | — | `true` | Master switch for mechanism b (initial-prompt biasing). |
| `biasMaxTerms` | int | `0..256` | `64` | Cap on terms injected into the initial prompt (keeps the prompt short). |

The Hub disables the fuzzy controls on an entry while the global fuzzy switch is off; the engine
re-checks both gates defensively in `apply_dictionary` (never trusts the UI).

---

## 5. Threading / Performance

This is a **pure, in-memory text transform** — cheap and off the audio thread.

- **Audio thread**: unaffected. Dictionary rewrite never runs in the cpal callback; it runs in the
  text stage after STT returns.
- **Warm model**: this feature does **not** load or cold-spawn any model. Mechanism (b) only
  contributes a precomputed initial-prompt string to the existing **warm** Whisper call
  ([ADR-004](architecture.md#adr-004-warmresident-stt-for-live-dictation)); `build_bias_prompt`
  runs once per utterance (or is cached and invalidated on dictionary edits).
- **Latency budget**: `apply_dictionary` is O(tokens × entries) in the simple form, sub-millisecond
  for realistic dictionary sizes (hundreds of terms, short utterances). It is **not** on the audio
  hot path and adds no perceptible delay to the utterance-end → first-injected-char budget. For
  large dictionaries the matcher can be backed by a precompiled set/trie built once on load.
- **Cancellation**: if the in-flight utterance is cancelled (hotkey abort), the cleaned text is
  discarded before the dictionary stage runs — nothing to undo.
- **Resource use**: the dictionary is a small `Vec`/index in managed `State` (kilobytes). No model
  RAM, no download. Edits rewrite the JSON file atomically and refresh the in-memory index.

---

## 6. UI States

This feature has **no HUD presence** — the dictionary rewrite is invisible during dictation (text
just comes out correct). All UI lives in the **Settings/Hub window** (light theme; see
[settings.md](settings.md) and [design-system.md](design-system.md)).

```
Hub "Dictionary" panel:
  Empty(no terms) → List(terms) ⇄ EditEntry(add/edit form) → Saving → List | ValidationError(inline)
  Import/Export:  List → Importing → List(merged) | ImportError
```

- **HUD** (while dictating): **none** — the rewrite is silent; the user sees correct text appear at
  the cursor like any other dictation ([tray-and-hud.md](tray-and-hud.md)).
- **Settings/Hub**: a searchable term list (Card), an add/edit Field form with toggles for
  `caseSensitive` / `wholeWord` / `fuzzy` / `biasPrompt` (Toggle), inline validation errors, and
  Import/Export actions. Empty state explains the feature with one example. Global fuzzy/bias
  switches live in a small "Matching" subsection.
- One action-blue accent only; hit targets ≥40px; validation errors use text + icon, not color
  alone (accessibility).

---

## 7. Edge Cases

| Scenario | Expected behavior |
|---|---|
| Two entries match overlapping spans | Longest match wins; tie → exact-over-fuzzy, then entry order; no re-match inside a replaced span (Rule 9). |
| Term is a substring of a longer word (`cat` in `category`) | No match with `wholeWord=true` (default) — boundary rule prevents partial-word hits (Rule 5). |
| Variant clashes with a common word, lowercase | Use `caseSensitive=true` so only the exact-case form (`"IT"`) matches, not `"it"` (Rule 3). |
| Fuzzy would rewrite a legitimate different word | Bounded by `fuzzyMaxDistance` (≤1 default) + length floor (≥4 chars) + per-entry + global gate; exact matches always win (Rules 7-8). |
| Output of a replacement could re-trigger another entry | Idempotent scan; replaced spans are skipped; no cascading/looping rewrites (Rule 10). |
| pt-BR accented term (`Tchéka`, `não`) | Unicode-aware boundaries and matching; accents are word characters; `replacement` emitted verbatim. |
| Duplicate variant added | `dict_add`/`dict_update` reject with `"duplicate term: <variant>"` (Rule 12). |
| Malformed import JSON | `dict_import` returns `Err("invalid dictionary json: …")`; existing dictionary untouched. |
| Biasing prompt grows huge | Capped at `biasMaxTerms` (default 64); excess terms still enforced by mechanism (a) (Rule 13). |
| Biasing disabled or unsupported by backend | Mechanism (a) still enforces every term — full correctness without (b) (Scope decision 1). |
| Empty / all-disabled dictionary | No-op; input returned unchanged (Rule 11). |

---

## 8. Testing Checklist

- **Rust** (`cargo test`, no I/O — pure helpers only):
  - [x] `apply_dictionary`: exact, case-insensitive, case-sensitive, whole-word vs partial, multi-word
        phrase variants (Rules 1-6).
  - [x] `match_case`: verbatim brand output, sentence-start capital carry-over (Rules 2-4).
  - [x] `fuzzy_match`: hits within distance, misses beyond it, short-token floor, exact-over-fuzzy
        priority (Rules 7-8).
  - [x] overlap/longest-match resolution and no-re-match-in-replaced-span (Rule 9).
  - [x] idempotency: `apply_dictionary(apply_dictionary(x)) == apply_dictionary(x)` (Rule 10).
  - [x] disabled/empty dictionary is a no-op (Rule 11).
  - [x] `validate_entry`: empty replacement, over-length → correct `Err(String)` (Rule 12). (Duplicate-variant rejection is command-level, pending CRUD.)
  - [x] `build_bias_prompt`: respects `biasEnabled`, `biasPrompt` per entry, and `biasMaxTerms` cap (Rule 13).
- **Manual / runtime** (needs mic, model, a real focused app):
  - [ ] add a term, dictate it (pt-BR and English), confirm correct spelling/casing appears at cursor.
  - [ ] case-sensitive term does not clobber the common lowercase word.
  - [ ] fuzzy entry corrects a realistic near-miss without breaking neighboring words.
  - [ ] biasing on vs off: term recognized more reliably with biasing, still corrected either way.
  - [ ] import/export round-trips the dictionary; validation errors surface inline in the Hub.

---

## 9. Out of Scope (this version)

- **Auto-learn from corrections** — inferring new dictionary entries by observing how the user edits
  MIA's output (e.g. they retype `"Tcheka"` → `"Tchéka"` repeatedly) and suggesting an entry. Noted
  as a desirable later enhancement; deferred to a future personalization iteration / backlog
  ([../ROADMAP.md](../ROADMAP.md) Phase 5).
- **LLM-assisted disambiguation** — using the optional local LLM ([ai-commands.md](ai-commands.md))
  to resolve ambiguous corrections. The dictionary stays deterministic (ADR-008); LLM help is a
  separate feature.
- **Cloud / cross-device sync of the dictionary** — never; the dictionary is local-only
  ([ADR-001](architecture.md#adr-001-native-on-device-privacy-first)). Import/Export of the JSON
  file is the supported sharing/backup path.
- **Per-app or per-context dictionaries** — a single global dictionary in v1; per-app writing
  styles/context are tracked under Phase 3 personalization more broadly (see
  [../ROADMAP.md](../ROADMAP.md)).
- **Regex / programmable replacement rules** — only literal terms + bounded fuzzy in v1; richer
  patterns belong to [snippets.md](snippets.md) (voice-triggered expansion) or a later iteration.
