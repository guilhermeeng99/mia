# Text Cleanup Feature Spec

> **Status**: Phase 1 — implemented: pure `cleanup.rs` with per-language (pt-BR / En / Other) rule sets — spoken-punctuation substitution, filler removal, repeat collapse, whitespace normalization, capitalization fix, light number spacing, optional trailing period — all cargo-tested. The pure `clean()` entry point is complete; pending only: wire it into the live dictation orchestrator + expose the cleanup toggles in Hub settings.
> **Last updated**: 2026-05-29
> **Coverage**: Sections 1-9 drafted.
> **Environment**: desktop (Windows, native)

The **deterministic cleanup** stage sits between STT and injection in the dictation pipeline
(hotkey → capture → VAD → STT → **cleanup** → inject) and runs on **every** utterance. It is a
**pure Rust module** — `app/src-tauri/src/cleanup.rs` — that takes raw Whisper output and returns
faithful, polished text **without a neural model** and **without inventing content**: it removes
filler words, substitutes spoken punctuation ("ponto" → `.`), collapses stutters and false
starts, normalizes whitespace, and fixes sentence-case. It is the **always-on, fidelity-safe**
first tier of MIA's text intelligence ([ADR-008](architecture.md#adr-008-hybrid-text-intelligence--deterministic-cleanup-phase-1--optional-local-llm-phase-2)),
landing in **Phase 1** (see [../ROADMAP.md](../ROADMAP.md)). The smarter, *rewriting* tier — the
opt-in LLM "Polish" and Command Mode — is **Phase 2** and explicitly out of scope here
([ai-commands.md](ai-commands.md)). Because every rule is a pure, total string→string function,
the whole module is exhaustively `cargo test`-able — this is precisely the pure-helper pattern.

**Scope decisions** (locked at design time):

- **Deterministic and rule-based only — no model, no inference** (ADR-008 / Phase 1). Cleanup
  must be debuggable, reproducible, and **sub-millisecond**; the same input always yields the same
  output. Anything requiring a model (rephrasing, formalizing, voice editing) is **Phase 2**
  ([ai-commands.md](ai-commands.md)).
- **Zero hallucination — never adds words, only removes/substitutes/recases existing ones.** The
  pipeline's faithful-not-creative default ([ADR-001](architecture.md#adr-001-native-on-device-privacy-first))
  starts at STT (anti-hallucination flags, [speech-to-text.md](speech-to-text.md)) and is upheld
  here: cleanup may delete filler, replace a recognized punctuation word, or change case — it may
  **never** introduce content the speaker did not say.
- **Per-language rule sets, selected by the STT-detected (or forced) language.** Filler lists and
  spoken-punctuation maps differ per language; pt-BR and English are first-class, others fall back
  to a language-agnostic core (whitespace/casing only). Language is an **input**, not guessed here.
- **Conservative by default — when in doubt, keep the text.** Over-deletion is worse than
  under-deletion (a stray "like" left in is better than a deleted real word). Filler removal is
  position- and context-guarded (Rule 2) so legitimate uses survive.
- **Runs *before* the custom dictionary, *separate* from it.** Generic, language-level fixes live
  here; **user-specific** word replacements and vocabulary live in
  [custom-dictionary.md](custom-dictionary.md). Cleanup does not know the user's jargon and must
  not strip it.

---

## 1. Inputs / Outputs

This stage is **language-agnostic plumbing over a per-language rule set**: text in, text out, no
mic, no model.

| Aspect | This feature |
|---|---|
| **Trigger** | Internal — called by the dictation orchestrator after STT returns a transcript ([dictation.md](dictation.md)). Not a user-facing command. |
| **Audio in** | N/A — runs after STT. |
| **Text in** | Raw UTF-8 transcript string from the warm Whisper model (one utterance). |
| **Text out** | Cleaned UTF-8 string, ready for the custom-dictionary pass then injection at the cursor. |
| **Target** | The orchestrator (which forwards the result to [text-injection.md](text-injection.md)). |
| **Language** | pt-BR / English first-class; any other Whisper language → language-agnostic core only. The language tag comes **in** (STT-detected or user-forced), never inferred here. |

No engine, crate, or I/O is involved: pure Rust string processing, in MIA's own process, on a
worker thread. The audio buffer never reaches this stage and never touches disk (ADR-001).
Latency budget: **sub-millisecond** for a normal utterance — negligible against STT inference.

---

## 2. Engine Contract (Rust)

Cleanup is a **pure helper module** — no `#[tauri::command]`, no `State`, no I/O. The orchestrator
([dictation.md](dictation.md)) calls it in-process; the Svelte UI never calls it directly. Its
*settings* (which toggles are on) are persisted/read through the settings command group
([settings.md](settings.md)); the cleanup module itself just takes an `options` struct.

**Module**: `app/src-tauri/src/cleanup.rs`

```rust
/// Detected or user-forced language for rule-set selection.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lang { PtBr, En, Other }   // `Other` → language-agnostic core only

#[derive(Clone)]
pub struct CleanupOptions {
    pub remove_fillers: bool,         // default true
    pub spoken_punctuation: bool,     // default true
    pub collapse_repeats: bool,       // default true
    pub fix_capitalization: bool,     // default true
    pub normalize_numbers: bool,      // default true (light)
    pub ensure_trailing_period: bool, // default false
    pub extra_fillers: Vec<String>,   // user-added stoplist entries (lowercased)
    pub keep_fillers: Vec<String>,    // user allow-list (never strip these)
}

impl Default for CleanupOptions { /* the defaults above */ }

/// The single public entry point. Pure, total, no I/O, no panic.
/// raw → polished. Empty/whitespace-only input → "".
pub fn clean(text: &str, lang: Lang, opts: &CleanupOptions) -> String;
```

`clean` is composed of small **pure stage functions**, each independently `cargo test`-ed and each
mapping 1:1 to a Business Rule below. They run in a fixed order (Rule 9):

```rust
fn substitute_spoken_punctuation(s: &str, lang: Lang) -> String;  // Rule 1
fn remove_fillers(s: &str, lang: Lang, opts: &CleanupOptions) -> String; // Rule 2
fn collapse_repeats(s: &str) -> String;                            // Rule 3
fn normalize_whitespace(s: &str) -> String;                        // Rule 4
fn fix_capitalization(s: &str, lang: Lang) -> String;              // Rules 5-6
fn normalize_numbers(s: &str, lang: Lang) -> String;               // Rule 7
fn ensure_trailing_period(s: &str) -> String;                      // Rule 8

// Per-language data (pure consts/statics, also test-asserted):
fn filler_set(lang: Lang) -> &'static [&'static str];
fn punctuation_map(lang: Lang) -> &'static [(&'static str, Punct)];
```

- **No `Err` path of its own.** `clean` is total and infallible — it cannot fail, so it returns a
  plain `String`, not `Result`. The orchestrator's command still returns `Result<T, String>`
  (ADR-006); cleanup never contributes an error.
- **No native crate, no library, no sidecar, no DLL.** Standard-library string handling only
  (plus a small regex/`unicode-segmentation` for word boundaries if needed) — all permissive
  (ADR-010).
- **Pure helpers behind `#[cfg(test)]`.** Every stage function and both per-language data tables
  are exercised by table-driven `cargo test`s (Section 8) — no mic, no model, no disk.
- **UI wrapper**: there is no dedicated `cleanup.ts`; toggles live in the settings group
  (`app/src/lib/settings.ts`, see [settings.md](settings.md)) and flow into `CleanupOptions`.

---

## 3. Business Rules

Numbered, testable, unambiguous. Each rule is one or more `cargo test` cases over its pure stage
function. Throughout: **never add words; when uncertain, keep the text.**

1. **Spoken-punctuation substitution.** Recognized punctuation *words* spoken by the user are
   replaced by the corresponding glyph and spacing, per the language map:
   - pt-BR: `ponto` → `.` · `vírgula` → `,` · `ponto e vírgula` → `;` · `dois pontos` → `:` ·
     `ponto de interrogação` / `interrogação` → `?` · `ponto de exclamação` / `exclamação` → `!` ·
     `reticências` → `…` · `abre parênteses` → `(` · `fecha parênteses` → `)` · `abre aspas` /
     `fecha aspas` → `"` · `travessão` / `hífen` → `—` / `-` · `nova linha` → `\n` ·
     `novo parágrafo` → `\n\n`.
   - en: `period` / `full stop` → `.` · `comma` → `,` · `semicolon` → `;` · `colon` → `:` ·
     `question mark` → `?` · `exclamation mark` / `exclamation point` → `!` · `ellipsis` → `…` ·
     `open paren` / `open parenthesis` → `(` · `close paren` → `)` · `open quote` / `close quote`
     → `"` · `dash` / `hyphen` → `—` / `-` · `new line` → `\n` · `new paragraph` → `\n\n`.
   Substitution attaches the glyph to the preceding word (no space before `.`/`,`/`?`/`!`/`;`/`:`),
   leaves one space after, and is **whole-token, case-insensitive** — never matches inside a word
   (e.g. en "comma" must not fire on "command"; a substring is never replaced). Longer phrases win
   over shorter (`novo parágrafo` before `nova linha`; `ponto e vírgula` before `ponto`).
2. **Filler-word removal (context-guarded).** Tokens in the language stoplist (plus
   `opts.extra_fillers`, minus `opts.keep_fillers`) are removed when they act as fillers:
   - pt-BR stoplist: `é` (as hesitation), `eh`, `ah`, `hum`, `tipo`, `tipo assim`, `né`, `sabe`,
     `então` (only as a sentence-initial/standalone filler), `assim`, `meio que`.
   - en stoplist: `um`, `uh`, `er`, `erm`, `hmm`, `like` (only as discourse filler), `you know`,
     `i mean`, `kind of` / `sort of` (as hedges), `basically` (as filler), `actually` (as filler).
   - **Guards against over-deletion** (each a test case): only remove a filler token when it is
     delimited as a standalone word; **never** remove it mid-clause where it carries meaning —
     pt-BR `é` as the verb *to be* ("ela é médica") is **kept**; en `like` as a verb/preposition
     ("I like coffee", "it works like this") is **kept**, only the discourse-marker use ("it's,
     like, broken") is dropped; `actually`/`basically` are kept when not sentence-adverbial filler.
     When a guard is ambiguous, **keep the word** (conservative default). Removal cleans up the
     resulting double space (Rule 4).
3. **Stutter / false-start / immediate-repeat collapse.** Consecutive duplicate tokens (and short
   dangling false starts) are collapsed to a single instance, case-insensitively: `the the cat` →
   `the cat`; `eu eu fui` → `eu fui`; broken-word stutters `th-the` / `wh-what` →
   `the` / `what`. Only **adjacent** repeats collapse; a repeat separated by other tokens
   (`good, very good` — non-adjacent) is **kept**. Repeats spanning a punctuation/sentence boundary
   are **not** collapsed (`good. good morning` stays). Note: deliberate adjacent emphasis
   ("very very") also collapses — the conservative tradeoff favours de-stuttering; a future opt-out
   is out of scope.
4. **Whitespace normalization.** Collapse runs of spaces/tabs to a single space; trim leading and
   trailing whitespace per line; remove spaces immediately *before* `.,;:?!` and *inside*
   parentheses/quotes; ensure exactly one space after sentence punctuation (none before a closing
   paren/quote); collapse 3+ blank lines to the paragraph break produced by Rule 1
   (`\n\n`). The output never has trailing spaces.
5. **Sentence-start capitalization.** The first alphabetic character of the text, and the first
   after a sentence terminator (`.`/`?`/`!`/`…`) or a paragraph break (`\n\n`), is uppercased.
   Capitalization is **case-correcting only** — it never changes interior casing of a word the
   user dictated (preserves acronyms, camelCase, and the custom dictionary's casing, which is
   applied later — [custom-dictionary.md](custom-dictionary.md)). A leading non-letter (digit,
   `(`) is skipped to the first letter.
6. **Pronoun "I" capitalization (en only).** The standalone English pronoun `i` is uppercased to
   `I` (whole-token, including contractions: `i'm` → `I'm`, `i'll` → `I'll`). This rule is **en
   only** — it must not fire for pt-BR (`Other` and `PtBr` skip it).
7. **Light number normalization.** A conservative, *non-rewriting* pass only: trim a space inside
   an obviously split decimal/ordinal where the language clearly intends it (pt-BR `vírgula`
   between digits from Rule 1 already yields `3,14`); normalize spacing around a digit-glyph
   produced by punctuation substitution. It does **not** convert spoken number words to digits
   ("twenty three" stays "twenty three") — that is ambiguous and rewriting-adjacent, deferred to
   Phase 2 ([ai-commands.md](ai-commands.md)). Default-on but intentionally minimal.
8. **Trailing-period handling.** If `ensure_trailing_period` is on and the final non-whitespace
   character is not already a terminator (`.?!…`) or a closing bracket/quote, append a single `.`.
   Off by default (many dictation targets — chat boxes, search fields — should not get a forced
   period). Never appends to empty output.
9. **Deterministic stage order.** Stages run in the fixed order in Section 2 — spoken punctuation
   first (so later stages see real `\n`/glyphs), then filler removal, repeat collapse, whitespace,
   capitalization, "I", numbers, trailing period. Given identical `(text, lang, opts)`, the output
   is **byte-for-byte identical** every time. Empty or whitespace-only input returns `""` and skips
   all stages.

---

## 4. Options & Defaults

Each rule group is independently toggleable so a user who wants raw-faithful output can disable any
cleanup they dislike. All toggles live in the Settings/Hub window ([settings.md](settings.md)) and
flow into `CleanupOptions`. (STT anti-hallucination flags are separate and **fixed**, ADR-007 — see
[speech-to-text.md](speech-to-text.md).)

| Option | Type | Range / values | Default | Effect |
|---|---|---|---|---|
| `remove_fillers` | bool | on/off | `true` | Apply the context-guarded filler stoplist (Rule 2). |
| `spoken_punctuation` | bool | on/off | `true` | Substitute spoken punctuation words for glyphs (Rule 1). |
| `collapse_repeats` | bool | on/off | `true` | Collapse adjacent stutters/false starts/repeats (Rule 3). |
| `fix_capitalization` | bool | on/off | `true` | Sentence-start casing + en `I` (Rules 5-6). |
| `normalize_numbers` | bool | on/off | `true` | Light, non-rewriting number spacing (Rule 7). |
| `ensure_trailing_period` | bool | on/off | `false` | Force a terminal `.` when missing (Rule 8). |
| `extra_fillers` | string[] | user-entered words | `[]` | Added to the stoplist (lowercased, whole-token). |
| `keep_fillers` | string[] | user-entered words | `[]` | Allow-list: never strip these (overrides the stoplist). |

Validation: the UI lowercases/trims `extra_fillers` and `keep_fillers` and rejects empties; the
engine re-applies whole-token matching defensively. Whitespace normalization (Rule 4) is **always
on** and not user-toggleable — it has no failure mode and no faithful-text cost. Language is not an
option here; it is supplied by the orchestrator from STT detection or the user's forced-language
setting ([speech-to-text.md](speech-to-text.md)).

---

## 5. Threading / Performance

- **Audio thread**: not involved — cleanup runs **after** capture and STT. It never touches the
  cpal real-time callback.
- **Warm model**: cleanup uses **no** model — it is the explicit *no-inference* tier. It runs on
  the same worker that produced the transcript, immediately after STT returns and before injection
  ([dictation.md](dictation.md)). It does **not** cold-spawn `whisper-cli` (it spawns nothing).
- **Latency budget**: **sub-millisecond** for an ordinary utterance (a handful of allocations and
  linear scans over a few hundred characters). It is effectively free against the dominant cost
  (STT inference), and is off the audio hot path entirely.
- **Cancellation**: not separately cancellable — it is synchronous and instantaneous. If the
  utterance is cancelled upstream (hotkey released/aborted), cleanup is simply never invoked; no
  partial cleaned text is produced or injected.
- **Resource use**: negligible — no model RAM, no download, nothing lazy-loaded. The per-language
  filler set and punctuation map are small `&'static` tables compiled into the binary.

---

## 6. UI States

Cleanup has **no runtime state machine of its own** — it is a synchronous transform inside the
dictation pipeline. During dictation the user sees the HUD's normal **Transcribing → Inserting**
sequence ([tray-and-hud.md](tray-and-hud.md)); the cleanup step is invisible (it adds no perceptible
delay and no distinct HUD state). The only surface is the **Settings/Hub window** (light theme), a
"Text cleanup" section.

```
States (pipeline, not owned here): … → Transcribing(spinner) → [cleanup runs] → Inserting(check) → Idle
Settings surface: Hub "Text cleanup" panel — toggles (Section 4) + a live preview.
```

- **HUD** (while dictating): no dedicated state — cleanup is part of the gap between Transcribing
  and Inserting. Keep the one-action-color discipline; nothing flashes for this step.
- **Settings/Hub**: the "Text cleanup" panel lists each toggle (Section 4) with a one-line
  explanation, plus the `extra_fillers` / `keep_fillers` editors and a **live before/after
  preview** (a sample raw transcript run through `clean` with the current options) so the user can
  see exactly what each toggle does — reinforcing *deterministic & debuggable*. Empty/loading/error
  are trivial (settings load locally). Hit targets ≥40px; toggles labelled (not color-only).

---

## 7. Edge Cases

| Scenario | Expected behavior |
|---|---|
| Empty / whitespace-only input | Return `""`; skip all stages; never inject (no forced period). |
| All-filler utterance ("um... uh... like...") | After removal, output is empty or whitespace → return `""`; orchestrator injects nothing (no empty/garbage text). |
| Filler word used legitimately (pt-BR "ela **é** médica"; en "I **like** coffee", "works **like** this") | **Kept** — Rule 2 guards remove only the discourse-marker/hesitation use; ambiguous → keep. |
| Code / technical dictation ("for each item, like so") | Conservative guards keep real "like"/"basically"; user can also add them to `keep_fillers`. Spoken punctuation still applies (e.g. "open paren"). |
| Punctuation word inside a real word (en "command", "period costume"; pt-BR "apontamento") | Whole-token, case-insensitive match only — substring never substituted (Rule 1). |
| Custom-dictionary terms / jargon | Untouched here; the dictionary pass runs **after** cleanup and owns user vocabulary ([custom-dictionary.md](custom-dictionary.md)). |
| Mixed-language utterance | Uses the single supplied `lang` rule set; the other language's fillers/punctuation are left as-is (faithful, no guessing). |
| Already-clean text | Idempotent-ish: running `clean` adds no content; re-running on cleaned output yields the same string (no double-capitalization, no stray periods). |
| User disables every toggle | Only the always-on whitespace normalization (Rule 4) applies; otherwise output ≈ raw transcript. |
| Very long utterance | Linear-time stages; still sub-millisecond-class; no caps needed (STT already bounds length). |

---

## 8. Testing Checklist

- **Rust** (`cargo test`, no I/O — this module is *all* pure helpers):
  - [ ] `substitute_spoken_punctuation` — each pt-BR and en mapping; whole-token only (not inside
        "command"/"apontamento"); longest-match wins (`novo parágrafo` vs `nova linha`,
        `ponto e vírgula` vs `ponto`); correct spacing around the inserted glyph.
  - [ ] `remove_fillers` — each stoplist entry removed as filler; **kept** in legitimate uses
        (`é`=verb, `like`=verb/prep, `actually`/`basically` non-filler); `extra_fillers` added;
        `keep_fillers` overrides; ambiguous→keep.
  - [ ] `collapse_repeats` — adjacent duplicates collapse; broken-word stutters (`th-the`);
        non-adjacent repeats and cross-sentence repeats preserved.
  - [ ] `normalize_whitespace` — multi-space/tab collapse, trim, no space before `.,;:?!`, paragraph
        breaks, no trailing spaces.
  - [ ] `fix_capitalization` — first letter + post-terminator + post-paragraph; interior casing /
        acronyms preserved; leading non-letter skipped.
  - [ ] en `I` rule — `i`/`i'm`/`i'll` → `I…`; does **not** fire for `PtBr`/`Other`.
  - [ ] `normalize_numbers` — light spacing only; does **not** convert "twenty three" → "23".
  - [ ] `ensure_trailing_period` — appends when on & missing; no double terminator; off→untouched;
        empty→untouched.
  - [ ] `clean` end-to-end — fixed stage order; **determinism** (same input → byte-identical output
        across runs); empty/whitespace → `""`; all-filler → `""`; idempotency on already-clean text;
        every-toggle-off ≈ raw + Rule 4.
  - [ ] per-language data tables (`filler_set`, `punctuation_map`) — present/non-empty for PtBr/En,
        language-agnostic core for `Other`.
- **Manual / runtime** (in the real pipeline):
  - [ ] dictate pt-BR with spoken punctuation ("abre parênteses … fecha parênteses ponto") → correct
        glyphs injected at cursor.
  - [ ] dictate en with fillers ("um, so, like, the thing is …") → fillers gone, real words intact.
  - [ ] Hub live preview reflects each toggle change immediately and matches injected output.
  - [ ] all-filler utterance injects nothing; legitimate "I like coffee" survives.

## 9. Out of Scope (this version)

- **LLM-based smarter cleanup / rewriting** — formalizing tone, restructuring sentences, fixing
  grammar beyond casing, converting number words to digits: all **Phase 2**, the opt-in "Polish"
  action and Command Mode via the local LLM ([ai-commands.md](ai-commands.md)). This module stays
  deterministic and non-rewriting.
- **User-specific vocabulary / word replacement** — personal jargon, name spellings, brand casing:
  owned by the custom dictionary, applied as a separate pass after cleanup
  ([custom-dictionary.md](custom-dictionary.md)).
- **Voice-triggered text expansion** — snippets/macros ([snippets.md](snippets.md)).
- **Per-app / per-context writing styles** — Phase 3 personalization ([../ROADMAP.md](../ROADMAP.md)).
- **Languages beyond pt-BR/English** get only the language-agnostic core (whitespace + sentence-case
  by terminator); full filler/punctuation rule sets for other Whisper languages are backlog.
