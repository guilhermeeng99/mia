# Snippets Feature Spec

> **Status**: Phase 3 — pure core implemented & cargo-tested in `snippets.rs`: `expand_snippets` (whole-phrase, word-boundary, longest-first, no recursion, verbatim expansion), `compile_snippets`, `normalize_trigger` (case + accent fold via NFD), `apply_case`, `validate_snippet` (Rules 1-11). CRUD commands (`list_snippets`/`upsert_snippet`/`delete_snippet`/`preview_expansion`) + atomic `snippets.json` persistence + managed state + cross-trigger duplicate rejection (`duplicate_trigger`) + `snippets.ts` wrapper are implemented (build-verified). The Hub snippets section (add / remove / live preview) is built + build-verified. The master `snippets_enabled` toggle is wired into the dictation pipeline: `settings.rs` defines `snippets_enabled` (default `true`) and `dictation.rs` gates snippet expansion on `settings.general.snippets_enabled`.
> **Last updated**: 2026-05-29
> **Coverage**: Sections 1-9 drafted
> **Environment**: desktop (Windows, native)

Snippets are user-defined **voice-triggered text expansions**: the user speaks a short *trigger
phrase* (e.g. "insert my signature", "minha assinatura", "endereço comercial") and MIA replaces it
with a longer canned *expansion* — an email signature, a postal address, a boilerplate paragraph, a
meeting link. Expansion is a **deterministic, pure** transformation applied to the *text* late in the
dictation pipeline (hotkey → capture → VAD → STT → **cleanup → custom-dictionary → snippets** →
inject): it runs **after** text-cleanup ([text-cleanup.md](text-cleanup.md)) and the custom
dictionary ([custom-dictionary.md](custom-dictionary.md)), and **before** injection
([text-injection.md](text-injection.md)). No microphone, no model, no network are involved — snippets
operate only on the already-transcribed string. The feature lands in **Phase 3 — Personalization**
(see [../ROADMAP.md](../ROADMAP.md)) alongside the custom dictionary, and implements **ADR-008**'s
deterministic-first principle (rule-based text intelligence, no LLM; see
[architecture.md](architecture.md#adr-008-hybrid-text-intelligence)). Snippets are managed from the
Hub ([settings.md](settings.md)).

**Scope decisions** (locked at design time):

- **Deterministic, rule-based, pure expansion — no LLM** — matching and substitution are a pure
  string transform with `#[cfg(test)]` cargo tests, on MIA's always-on fidelity-safe path. Voice
  *editing* / generative commands are the separate AI Command Mode (Phase 2,
  [ai-commands.md](ai-commands.md)); snippets never invent text (ADR-008 / Phase 3).
- **Snippets run after cleanup and after the custom dictionary** — the trigger is matched against the
  *cleaned, dictionary-corrected* transcript so a trigger written as "GitHub" still matches when the
  dictionary maps "github" → "GitHub". Fixed pipeline order; not user-reorderable (see Rule 1 and
  [text-cleanup.md](text-cleanup.md) / [custom-dictionary.md](custom-dictionary.md)).
- **Trigger matching is phrase-level and word-boundary aware** — a trigger matches a whole-word
  phrase, optionally anchored to the start of the utterance (configurable per snippet), never a
  substring inside a larger word. This avoids accidental expansion mid-word (Rule 4).
- **Expansion text is stored and injected verbatim** — the expansion is treated as literal text
  (including newlines, punctuation, URLs); cleanup is **not** re-run over it (it is already
  authored), so a signature's deliberate line breaks survive (Rule 6).
- **Storage is a single JSON file in app-data, plain text** — snippets are user content, not secrets;
  stored next to the dictionary and settings, never synced to a server (ADR-001).
- **No nesting / recursion** — an expansion is inserted literally and is **not** re-scanned for
  further triggers (no snippet-inside-snippet). Bounds latency and removes loops (Rule 7).
- **Dictionary takes precedence over snippets on collision** — if a phrase is both a dictionary entry
  and a snippet trigger, the dictionary substitution happens first (earlier stage); the snippet then
  matches whatever the dictionary produced. Documented, deterministic ordering (Rule 9).

---

## 1. Inputs / Outputs

This stage is past STT — it transforms text, it does not touch the mic or a model.

| Aspect | This feature |
|---|---|
| **Trigger** | Called by the dictation orchestrator on the cleaned, dictionary-corrected transcript (in-process Rust); plus Hub CRUD actions and a "test expansion" preview |
| **Audio in** | N/A — past STT |
| **Text in** | Cleaned + dictionary-corrected UTF-8 `String` (output of [custom-dictionary.md](custom-dictionary.md)); plus the loaded snippet set |
| **Text out** | The same string with any matched trigger phrases replaced by their expansions; passed on to injection ([text-injection.md](text-injection.md)). On the CRUD path: snippets persisted to app-data |
| **Target** | The cleaned-text buffer in the orchestrator (then the focused window, via injection); the Hub for management |
| **Language** | pt-BR and English (and any language) — triggers are user-authored literal phrases; matching is Unicode/diacritic-aware, not language-specific |

No backing engine crate beyond the standard library + `serde`/`serde_json` for persistence: snippet
expansion is **pure Rust** string work. Nothing here touches the network or the model. The snippet
set is loaded from disk **once** at startup / on edit and held in memory; the per-utterance
`expand_snippets` call performs **no** disk I/O (see Section 5). The transcript itself is never
written to disk (ADR-001); only the user-authored snippet definitions persist.

---

## 2. Engine Contract (Rust)

Rust is the **engine**; the Svelte UI is a thin webview that calls typed `invoke()` wrappers and
holds no expansion logic (see [architecture.md](architecture.md)). All commands return
`Result<T, String>` — no panics across the IPC boundary (ADR-006). In live dictation the orchestrator
calls `expand_snippets` **directly in Rust** on the cleaned text; the `#[tauri::command]`s exist for
the Hub's CRUD and preview.

**Module**: `app/src-tauri/src/snippets.rs`

```rust
/// How a trigger may match within an utterance.
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SnippetAnchor { Anywhere, StartOnly }   // default Anywhere

/// Optional case transform applied to the expansion when inserted.
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SnippetCase { Verbatim, MatchSentence }  // default Verbatim

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snippet {
    pub id: String,            // stable uuid
    pub trigger: String,       // spoken phrase, e.g. "minha assinatura"
    pub expansion: String,     // literal replacement (may contain \n, URLs)
    pub anchor: SnippetAnchor, // default Anywhere
    pub case: SnippetCase,     // default Verbatim
    pub enabled: bool,         // default true
}

/// The compiled, in-memory snippet set (longest-trigger-first for matching).
pub struct SnippetSet { /* Vec<CompiledSnippet> sorted by trigger word-len desc */ }

#[tauri::command]
fn list_snippets(state: State<'_, AppState>) -> Result<Vec<Snippet>, String>;

#[tauri::command]
fn upsert_snippet(state: State<'_, AppState>, snippet: Snippet) -> Result<Snippet, String>;

#[tauri::command]
fn delete_snippet(state: State<'_, AppState>, id: String) -> Result<(), String>;

#[tauri::command]
fn preview_expansion(
    state: State<'_, AppState>,
    text: String,                 // sample utterance
) -> Result<ExpansionResult, String>;   // { output, appliedTriggers: Vec<String> }
```

- **`expand_snippets(text, &SnippetSet) -> ExpansionResult`** — the **pure** core (not a command;
  called in-process by the orchestrator and by `preview_expansion`). Scans the cleaned text for
  whole-phrase, word-boundary trigger matches and replaces each with its expansion; returns the new
  string plus the list of triggers applied. No I/O.
- **`list_snippets` / `upsert_snippet` / `delete_snippet`** — Hub CRUD; load/modify/save the JSON
  store and rebuild the in-memory `SnippetSet`. `upsert_snippet` validates (non-empty trigger,
  non-empty expansion, no duplicate-trigger conflict) and returns `Err(String)` on bad input.
- **`preview_expansion`** — runs `expand_snippets` over a sample string so the Hub can show the user
  exactly what their snippets will do, without dictating.
- **`Err(String)` messages** (map 1:1 to UI states): `"trigger cannot be empty"`,
  `"expansion cannot be empty"`, `"a snippet with this trigger already exists"`,
  `"snippet not found"`, `"failed to read snippets file"`, `"failed to write snippets file"`.
- **Pure helpers** (no I/O, behind `#[cfg(test)]` cargo tests):
  - `compile_snippets(&[Snippet]) -> SnippetSet` — filters `enabled`, normalizes triggers (trim,
    NFC, fold to a matching key), sorts **longest-trigger-first** so the most specific phrase wins.
  - `normalize_trigger(s: &str) -> String` — lowercase + NFC + collapse internal whitespace; this is
    the comparison key (matching is case- and accent-fold-insensitive on the trigger, per Rule 3).
  - `find_match(tokens: &[Token], set: &SnippetSet, anchor) -> Option<Match>` — word-boundary,
    longest-first phrase match over tokenized text.
  - `apply_case(expansion: &str, case: SnippetCase, ctx) -> String` — applies `MatchSentence`
    capitalization or returns the expansion verbatim.
  - `expand_snippets(...)` itself is pure and fully unit-tested.
- **UI wrapper**: `app/src/lib/snippets.ts` — `invoke<Snippet[]>("list_snippets")`,
  `invoke<Snippet>("upsert_snippet", { snippet })`, `invoke<void>("delete_snippet", { id })`,
  `invoke<ExpansionResult>("preview_expansion", { text })`. The UI holds **no** matching logic.

---

## 3. Business Rules

1. **Fixed pipeline position** — snippet expansion runs on the transcript **after** text-cleanup and
   **after** the custom dictionary, and **before** injection. The order
   cleanup → dictionary → snippets → inject is fixed and not user-reorderable (see
   [text-cleanup.md](text-cleanup.md), [custom-dictionary.md](custom-dictionary.md),
   [text-injection.md](text-injection.md)).
2. **A trigger expands to its expansion verbatim** — when a trigger phrase matches, MIA replaces the
   matched span with the snippet's `expansion` string exactly as authored, including embedded
   newlines, punctuation, and URLs.
3. **Matching is case- and accent-fold-insensitive on the trigger** — "Minha Assinatura",
   "minha assinatura", and "MINHA ASSINATURA" all match a trigger stored as "minha assinatura"
   (`normalize_trigger` folds case + NFC). The **expansion** is unaffected (it is inserted per its
   `case` setting, default verbatim).
4. **Word-boundary, whole-phrase matching only** — a trigger matches only as a complete word/phrase
   bounded by whitespace or utterance edges; it never matches a substring inside a larger word
   (trigger "ass" must not fire inside "passar"). Multi-word triggers must match the words in order
   and adjacent.
5. **Anchor controls position** — `Anywhere` (default) matches the trigger anywhere in the utterance;
   `StartOnly` matches only when the trigger is the leading phrase of the utterance. A `StartOnly`
   trigger appearing mid-sentence does **not** expand.
6. **Expansion text is not re-cleaned** — cleanup rules (filler removal, spoken-punctuation, casing)
   are **not** re-applied to the inserted expansion; it is authored text and is inserted as-is.
   Surrounding (non-expanded) transcript text keeps whatever cleanup already produced.
7. **No recursion / no nesting** — an inserted expansion is **not** re-scanned for further triggers.
   A single pass replaces all top-level matches; expansions cannot contain or trigger other snippets.
8. **Multiple triggers in one utterance all expand** — every non-overlapping trigger match in the
   utterance is replaced in a single left-to-right pass. Once a span is consumed by an expansion,
   scanning resumes after it (overlapping matches are not double-expanded).
9. **Longest trigger wins on overlap; dictionary wins on cross-stage collision** — among snippet
   triggers, the longest (most words) matching phrase at a position is chosen (`compile_snippets`
   sorts longest-first). Across stages, the **custom dictionary runs first**, so if a phrase is both a
   dictionary entry and a snippet trigger, the dictionary substitution applies before snippets see the
   text (see [custom-dictionary.md](custom-dictionary.md)); the snippet then matches the dictionary's
   output, if at all.
10. **Disabled snippets never match** — `enabled = false` snippets are excluded by `compile_snippets`
    and have zero effect on expansion or on collision detection.
11. **Empty / whitespace input is a no-op** — `expand_snippets("")` returns the input unchanged with
    an empty `appliedTriggers` list; no error.
12. **Trigger and expansion must be non-empty on save** — `upsert_snippet` rejects an empty/whitespace
    trigger (`"trigger cannot be empty"`) or empty expansion (`"expansion cannot be empty"`).
13. **Duplicate triggers are rejected on save** — two enabled snippets may not share the same
    normalized trigger; `upsert_snippet` returns `"a snippet with this trigger already exists"` (the
    same `id` editing its own trigger is allowed).
14. **Spacing is preserved around the expansion** — replacing a trigger leaves exactly one boundary
    space on each side as it was in the source (no doubled or missing spaces); leading/trailing
    expansion whitespace defined by the user is respected.
15. **Optional case transform** — `case = MatchSentence` capitalizes the first letter of the expansion
    when it begins a sentence (start of utterance or after sentence-ending punctuation); `Verbatim`
    (default) inserts the expansion exactly as stored.

---

## 4. Options & Defaults

Global toggle plus per-snippet fields. Snippet content itself is user data (Section 2 model).

| Option | Type | Range / values | Default | Effect |
|---|---|---|---|---|
| `snippets_enabled` | bool | true/false | `true` | Master switch; when off, `expand_snippets` returns input unchanged |
| `Snippet.trigger` | string | non-empty, ≤120 chars | — | Spoken phrase to match (normalized for comparison, Rule 3) |
| `Snippet.expansion` | string | non-empty, ≤10000 chars | — | Literal replacement text, inserted verbatim (Rule 2) |
| `Snippet.anchor` | enum | `anywhere` / `startOnly` | `anywhere` | Where the trigger may match (Rule 5) |
| `Snippet.case` | enum | `verbatim` / `matchSentence` | `verbatim` | Capitalization of the inserted expansion (Rule 15) |
| `Snippet.enabled` | bool | true/false | `true` | Disabled snippets never match (Rule 10) |

Validation: the Hub disables Save when trigger or expansion is empty and surfaces duplicate-trigger
conflicts inline; the engine **re-validates** defensively in `upsert_snippet` (Rules 12-13). Length
caps are enforced UI-side and re-clamped engine-side. These are text-expansion options only — STT
anti-hallucination defaults remain fixed elsewhere (ADR-007; see
[speech-to-text.md](speech-to-text.md)).

---

## 5. Threading / Performance

- **Not on the audio thread** — expansion runs after STT + cleanup + dictionary, far off the cpal
  real-time callback (see [audio-capture.md](audio-capture.md)); it never blocks capture.
- **No model, no spawn, no I/O on the hot path** — `expand_snippets` is pure in-memory string work
  over the pre-compiled `SnippetSet`; it loads nothing and does not cold-spawn `whisper-cli` (the
  warm-model contract lives in [speech-to-text.md](speech-to-text.md) / ADR-004). Disk I/O happens
  only on CRUD (`upsert`/`delete`/load-at-startup), never per utterance.
- **Latency budget**: negligible (sub-millisecond for realistic snippet counts and utterance
  lengths). The dominant end-to-end cost remains STT inference; snippet expansion is off the critical
  path. The `SnippetSet` is compiled once on load/edit so per-utterance matching is a linear scan over
  tokens against a longest-first set.
- **Cancellation**: if the orchestrator cancels mid-utterance (hotkey released, abort), the cleaned
  text is discarded before snippets run — no stale expansion is produced or injected (see
  [dictation.md](dictation.md)).
- **Resource use**: trivial — the snippet set is small user-authored text held in memory; no GPU, no
  model RAM. The JSON store is read/written only on management actions.

---

## 6. UI States

Snippet expansion is invisible at runtime — it happens inside the **Transcribing → Inserting**
transition of the dictation HUD; the user just sees the expanded text appear at the cursor. The
management surface is the **Settings/Hub** window (light theme). See
[tray-and-hud.md](tray-and-hud.md) and [design-system.md](design-system.md).

```
Runtime (no dedicated HUD state):
… → Transcribing(spinner) → [cleanup → dictionary → expand_snippets] → Inserting(brief check) → Idle

Hub management:
List(empty | populated) → Editor(add/edit: trigger, expansion, anchor, case, enabled)
                        → Preview(type a sample → see output + applied triggers)
                        → Save(Ok → List | Err → inline message)  |  Delete(confirm → List)
```

- **HUD** (while dictating): no new state — expansion is silent; the result simply appears during
  `Inserting`. Keep the single action-blue accent discipline.
- **Settings/Hub** (light theme): a **Snippets** section with a searchable list (trigger →
  truncated expansion preview, enabled toggle), an add/edit editor (trigger field, multiline
  expansion field, anchor + case selectors, enabled toggle), and a **live Preview** box backed by
  `preview_expansion` showing the transformed sample and which triggers fired. Duplicate-trigger and
  empty-field errors show inline. ≥40px hit targets; don't rely on color alone (icon + text for
  enabled/disabled and error states).

---

## 7. Edge Cases

| Scenario | Expected behavior |
|---|---|
| Trigger inside a larger sentence | Expands in place; surrounding words preserved with correct spacing (Rules 8, 14) |
| Trigger appears as a substring of a longer word | No match — word-boundary, whole-phrase only (Rule 4) |
| Multiple triggers in one utterance | All non-overlapping matches expand, left-to-right, single pass (Rule 8) |
| Two triggers overlap at a position | Longest (most-words) trigger wins (Rule 9, `compile_snippets` longest-first) |
| Trigger collides with a custom-dictionary entry | Dictionary runs first; snippet matches dictionary's output (Rule 9; [custom-dictionary.md](custom-dictionary.md)) |
| Expansion text contains another trigger | Not re-scanned — no nesting/recursion (Rule 7) |
| `StartOnly` trigger appears mid-sentence | Does not expand (Rule 5) |
| Trigger differs only in case/accents from spoken form | Still matches — `normalize_trigger` folds case + NFC (Rule 3) |
| Duplicate trigger on save | `Err("a snippet with this trigger already exists")`, inline in Hub (Rule 13) |
| Empty trigger or empty expansion on save | `Err("trigger cannot be empty")` / `Err("expansion cannot be empty")` (Rule 12) |
| Snippet disabled | Excluded from matching and collision checks (Rule 10) |
| Master `snippets_enabled` off | `expand_snippets` returns input unchanged |
| Empty / whitespace utterance | Returned unchanged, empty `appliedTriggers` (Rule 11) |
| Snippets file missing / unreadable | Treat as empty set at load; `Err("failed to read snippets file")` only on an explicit list/CRUD call |

---

## 8. Testing Checklist

- **Rust** (`cargo test`, no I/O — pure helpers only):
  - [x] `expand_snippets` — single match anywhere; match at utterance start; no match for
        substring-inside-word; multiple non-overlapping matches in one utterance; empty/whitespace
        input unchanged
  - [x] `normalize_trigger` — case fold, NFC, internal-whitespace collapse; "Minha Assinatura" matches
        "minha assinatura"
  - [x] `compile_snippets` — longest-trigger-first ordering; disabled snippets excluded. (Duplicate
        normalized-trigger detection is command-level, pending CRUD.)
  - [x] `find_match` — word-boundary correctness; `StartOnly` matches only at the start; longest-first
        wins on overlap
  - [x] `apply_case` — `Verbatim` returns as-is; `MatchSentence` capitalizes at sentence start only
  - [x] expansion is **not** re-scanned (no recursion) and **not** re-cleaned (newlines/URLs intact)
  - [x] spacing preserved around an in-sentence expansion (no doubled/missing spaces)
  - [x] each `Err(String)` from `upsert_snippet`/`delete_snippet` — `validate_snippet` (empty trigger/
        expansion) + `duplicate_trigger` (cargo-tested) + upsert "snippet not found"; command paths build-verified.
- **Manual / runtime** (needs mic, model, a real focused app, and saved snippets):
  - [ ] happy path: speak a trigger → expansion typed at cursor (pt-BR and English triggers)
  - [ ] trigger embedded mid-sentence expands with surrounding text intact
  - [ ] multi-line signature expansion preserves its line breaks when injected
  - [ ] trigger that is also a dictionary entry behaves per Rule 9 ordering
  - [ ] Hub CRUD: add / edit / delete / toggle-enabled persists across restart
  - [ ] Hub Preview reflects exactly the runtime expansion (and lists applied triggers)
  - [ ] master `snippets_enabled` off → no expansion occurs

---

## 9. Out of Scope (this version)

- **Generative / dynamic expansions** (date, time, clipboard contents, AI-written text) — snippets are
  literal canned text only; voice editing and generation are AI Command Mode (Phase 2,
  [ai-commands.md](ai-commands.md)).
- **Nested / recursive snippets** — an expansion never triggers another snippet (Rule 7); deferred,
  likely permanently for safety.
- **Cursor placement / fields inside an expansion** (e.g. `{cursor}`, fill-in placeholders) —
  injection writes a flat string at the cursor and never moves it; potential Backlog item
  ([../ROADMAP.md](../ROADMAP.md)).
- **Per-app snippet sets / context** — target-specific expansions tie into per-app writing styles,
  also Phase 3+ ([../ROADMAP.md](../ROADMAP.md)); v1 of snippets is global.
- **Cloud sync / sharing of snippet libraries** — all snippet data stays local in app-data; no server
  (ADR-001). Import/export from a local file is a possible later convenience, not in this version.
- **Fuzzy / phonetic trigger matching** — matching is exact (normalized) whole-phrase only; fuzzy
  matching risks false expansions and is deferred.
