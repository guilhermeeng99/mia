# AI Commands (Command Mode + Polish) Feature Spec

> **Status**: ❌ **DESCOPED (product decision, 2026-05-29).** MIA stays a faithful,
> deterministic dictation tool — this local-LLM layer is not part of the product. A
> runtime (warm `llama-server` + GGUF download + `run_command`/`polish`) was built and
> then reverted; only the pure, cargo-tested helpers (`route_intent`, `command_grammar`,
> `build_prompt`, `validate_parsed`, `Intent`/`ParsedCommand`) remain **dormant** in
> `ai_commands.rs`, wired to nothing. This spec is retained as the design of record in
> case AI is ever reconsidered; nothing below is shipped.
> **Last updated**: 2026-05-29
> **Coverage**: Sections 1-9 drafted.
> **Environment**: desktop (Windows, native)

This is MIA's **Phase 2** text-intelligence tier — the *rewriting* half of the hybrid model in
[ADR-008](architecture.md#adr-008-hybrid-text-intelligence--deterministic-cleanup-phase-1--optional-local-llm-phase-2).
It adds an **optional, opt-in, lazily-loaded** small local LLM (via **llama.cpp** —
Qwen2.5-3B-Instruct or Llama-3.2-3B-Instruct at **Q4_K_M** GGUF, ~1.5-2 GB RAM, downloaded on
demand exactly like the Whisper models in [speech-to-text.md](speech-to-text.md)) that runs **only
when explicitly invoked**, so average dictation latency stays at Phase-1 levels. It exposes two
capabilities: (1) **Command Mode** — spoken instructions that *transform the last-inserted or
currently-selected text* ("make this more concise", "more formal", "turn into a bullet list",
"summarize", "translate to English"), parsed with **GBNF / JSON-schema constrained decoding** into
a structured `{action, target, params}` so even a 3B model can only emit a valid command; and
(2) **Polish** — an opt-in action that runs disfluency repair / grammar / list formatting / tone
adaptation over freshly dictated text. A cheap **intent router** decides *dictation vs command vs
polish* **before** the LLM is ever loaded or run. This tier sits **off** the always-on faithful
path ([dictation.md](dictation.md)): the default pipeline remains
hotkey → capture → VAD → STT → **deterministic cleanup** ([text-cleanup.md](text-cleanup.md)) →
inject, and the LLM is a deliberate, user-triggered detour from it. Lands in **Phase 2 — AI Magic**
(see [../ROADMAP.md](../ROADMAP.md)).

**Scope decisions** (locked at design time):

- **Opt-in, off by default, and never on the hot path** (ADR-008 / Phase 1 vs 2). The faithful
  STT → deterministic cleanup → inject path ([text-cleanup.md](text-cleanup.md)) is the default and
  is the *only* thing a fresh install does. The LLM is disabled until the user enables it in
  Settings ([settings.md](settings.md)) **and** downloads the model. A small 3B model
  overcorrects and confabulates (Section 7) — it must never silently rewrite faithful dictation.
- **Lazily loaded, runs only on explicit intent** (ADR-008). The model is **not** resident the way
  Whisper is ([ADR-004](architecture.md#adr-004-warmresident-stt-for-live-dictation)) — it is loaded
  on first use and may be unloaded on idle to reclaim RAM (Section 5). The **intent router** (Rule 2)
  is the gate: plain dictation never pays the LLM cost, so average latency tracks Phase 1.
- **Constrained decoding, not free-form generation, for Command Mode** (ADR-008). Command parsing
  uses a **GBNF grammar / JSON schema** so the model's output is *forced* into a valid
  `{action, target, params}` envelope. The LLM chooses *which* known action — it does not invent the
  command structure. Unknown/ambiguous → ask or no-op, never guess (Rule 7).
- **Transforms existing text; it does not transcribe.** Command Mode and Polish operate on text MIA
  already produced (last-inserted) or the user's selection — there is no separate audio path for the
  *content*; audio only carries the *instruction* (for Command Mode) and is transcribed by the same
  warm Whisper model. No new STT engine, no cloud (ADR-001).
- **Local-only, permissive-licensed model** (ADR-001 / ADR-010). llama.cpp (MIT), Qwen2.5 / Llama-3.2
  weights under their permissive community licenses; downloaded to app-data from Hugging Face,
  voice/text never leave the machine. Never bundle AGPL.

---

## 1. Inputs / Outputs

This feature is **text-in → text-out over a local LLM**, triggered either by a spoken command
(routed away from dictation) or by an explicit Polish action. Audio, when present, carries only the
*instruction*; the *content* is text MIA already has.

| Aspect | This feature |
|---|---|
| **Trigger** | (a) **Command Mode**: a spoken utterance the intent router classifies as a command (Rule 2) — e.g. a "Hey MIA, …"-style prefix or imperative phrasing while dictation is active; (b) **Polish**: an explicit user action — a HUD/tray button, a dedicated hotkey ([hotkeys.md](hotkeys.md)), or "polish that" command — never automatic. |
| **Audio in** | Only for Command Mode: the cpal 16 kHz mono PCM utterance carrying the *instruction*, transcribed by the warm Whisper model ([audio-capture.md](audio-capture.md), [speech-to-text.md](speech-to-text.md)). Polish takes no audio. The buffer stays in RAM, never on disk (ADR-001). |
| **Text in** | The **target text**: the last-inserted dictation result (held in `DictationState`, [dictation.md](dictation.md)) or the user's current selection (read via clipboard-copy, see Rule 9), **plus** the parsed instruction. |
| **Text out** | A transformed UTF-8 string. Default delivery: **replace** the target (select prior text → inject replacement, [text-injection.md](text-injection.md)); or **append**; the UI may offer accept/undo (Rule 10). Polish returns polished text the same way. |
| **Target** | The OS-focused window via the injection backend ([text-injection.md](text-injection.md)); transformations surface their state on the HUD ([tray-and-hud.md](tray-and-hud.md)). |
| **Language** | pt-BR and English first-class (prompts and few-shots per language); other Whisper languages best-effort. The instruction's language and the content's language are tracked separately (Rule 6). |

Backing engine: **llama.cpp** via the `llama-cpp-2` Rust bindings (in-process) by default, with a
**`llama-server`** (llama.cpp's OpenAI-compatible HTTP server) sidecar as the documented fallback —
mirroring the whisper-rs-in-process / whisper-server-fallback split in
[speech-to-text.md](speech-to-text.md). Model files (Q4_K_M GGUF, ~1.5-2 GB) are fetched on demand
to app-data and never bundled. Constrained decoding uses llama.cpp's native **GBNF grammar** support.

---

## 2. Engine Contract (Rust)

Implemented in `app/src-tauri` as the engine; the Svelte UI is a thin webview calling typed
`invoke()` wrappers (see [architecture.md](architecture.md)). All commands return
`Result<T, String>` — no panics across the IPC boundary
([ADR-006](architecture.md#adr-006-resultt-string-error-model-across-the-rust--ui-ipc)).

**Module**: `app/src-tauri/src/ai_commands.rs` (LLM runtime, model registry, prompt/grammar
builders, intent router) — orchestrated by `dictation.rs` ([dictation.md](dictation.md)).

```rust
/// Lazily-loaded local LLM. Lives in managed State; None until first use / after idle-unload.
struct LlmState {
    handle: Mutex<Option<LlmHandle>>, // loaded model + context (llama-cpp-2) or server client
    last_used: Mutex<Instant>,        // for idle-unload (Section 5)
}

#[derive(serde::Serialize, serde::Deserialize)] // rename_all = "camelCase"
enum Intent { Dictation, Command, Polish }       // router output (Rule 2)

#[derive(serde::Serialize, serde::Deserialize)] // the constrained Command envelope (Rule 4)
struct ParsedCommand {
    action: CommandAction,    // enum: Concise|Formal|Casual|BulletList|Summarize|Translate|Fix|Expand|Custom
    target: CommandTarget,    // enum: LastInserted | Selection
    params: CommandParams,    // e.g. { lang: Option<String>, instruction: Option<String> }
}

/// Cheap classification BEFORE the LLM is loaded — trigger-phrase + tiny heuristic/classifier.
/// Pure, no I/O, no model. Maps 1:1 to Rule 2. (#[cfg(test)] tested.)
fn route_intent(transcript: &str, lang: Lang, cfg: &AiConfig) -> Intent;

/// Build the GBNF grammar / JSON schema that constrains Command-Mode decoding. Pure. (tested)
fn command_grammar() -> &'static str;
/// Build the per-action, per-language prompt (system + few-shot + target text). Pure. (tested)
fn build_prompt(action: CommandAction, lang: Lang, target: &str, instr: Option<&str>) -> String;

#[tauri::command]
async fn ai_status() -> Result<AiStatus, String>;
//   { enabled, modelInstalled, modelId, loaded, vramOrRam, backend } — drives Settings + gating.

#[tauri::command]
async fn download_llm(model_id: String, on_progress: tauri::ipc::Channel<DownloadProgress>)
    -> Result<(), String>;
//   On-demand HF download (ureq, .part → rename), reusing Toolzy's transcription.rs pattern
//   (REUSE-FROM-TOOLZY.md). Same "download gate" UX as Whisper models.

#[tauri::command]
async fn run_command(state: State<'_, LlmState>, transcript: String, lang: String)
    -> Result<CommandResult, String>;
//   Command Mode: route_intent → (if Command) parse via grammar → apply transform → return
//   { newText, action, replacedRange }. Lazily loads the model.

#[tauri::command]
async fn polish(state: State<'_, LlmState>, text: String, lang: String, opts: Option<PolishOpts>)
    -> Result<PolishResult, String>;
//   Polish action over the supplied text. { polishedText }.

#[tauri::command]
async fn unload_llm(state: State<'_, LlmState>) -> Result<(), String>; // free RAM on demand
```

- **`Err(String)` cases** (each maps 1:1 to a UI state, Section 6): `"ai disabled"` (feature off),
  `"model not installed"` (gate → offer download), `"download failed: …"`, `"insufficient memory:
  needs ~2 GB free"`, `"model load failed: …"`, `"no target text"` (nothing to transform),
  `"command not understood"` (router/parse ambiguous, Rule 7), `"llm timeout"`, `"cancelled"`.
- **Native in-process** (`llama-cpp-2`) by default; **`llama-server` sidecar** fallback (configured
  via `externalBin` + scoped shell capability, as with whisper-server). Constrained decoding via
  llama.cpp GBNF in both backends.
- **Pure helpers behind `#[cfg(test)]`**: `route_intent`, `command_grammar`, `build_prompt`, the
  trigger-phrase tables (per language), the `ParsedCommand` (de)serialization, and a
  `validate_parsed(&ParsedCommand)` guard — all take no I/O and no model.
- **UI wrapper**: `app/src/lib/ai.ts` — `invoke<AiStatus>("ai_status")`,
  `invoke<CommandResult>("run_command", …)`, `invoke<PolishResult>("polish", …)`, download/unload.
  The UI holds **no** prompt, grammar, or routing logic.

---

## 3. Business Rules

Numbered, testable, unambiguous. Throughout: **the LLM is opt-in, off the default path, and never
rewrites faithful dictation unless the user explicitly asked.**

1. **Disabled and absent by default.** With AI features off (default) the router never runs and the
   model is neither downloaded nor loaded — dictation behaves exactly as Phase 1
   ([text-cleanup.md](text-cleanup.md)). Enabling the feature in Settings does **not** itself
   download the model; first actual use gates on the download (Rule 3, Edge Cases).
2. **Cheap intent routing before any LLM cost.** Every utterance produced while AI is enabled passes
   through the pure `route_intent` (no model, sub-millisecond): it returns `Command`, `Polish`, or
   `Dictation`. Routing uses (a) a configurable **trigger prefix** ("Hey MIA …" / a dedicated
   command hotkey/mode) and/or (b) a tiny imperative-phrase classifier (verb + "this/that/the
   text"). **Default-conservative**: anything not clearly a command/polish → `Dictation`, and the
   text flows down the faithful path untouched. The LLM is loaded **only** for `Command`/`Polish`.
3. **On-demand model download (gate).** `Command`/`Polish` requires the GGUF present in app-data. If
   missing, the action returns `Err("model not installed")` and the UI offers a one-tap download
   (`download_llm`, streamed progress) — the same gate UX as Whisper
   ([speech-to-text.md](speech-to-text.md)), reusing Toolzy's HF download pattern
   ([REUSE-FROM-TOOLZY.md](../REUSE-FROM-TOOLZY.md)). Dictation keeps working while it downloads.
4. **Constrained Command parsing into `{action, target, params}`.** Command Mode decodes under the
   GBNF grammar (`command_grammar`) so the model can emit **only** a valid `ParsedCommand` — a known
   `action` (Concise, Formal, Casual, BulletList, Summarize, Translate, Fix, Expand, Custom), a
   `target` (LastInserted or Selection), and `params` (e.g. `lang` for Translate, free-text
   `instruction` for Custom). Output that fails `validate_parsed` is rejected, not applied (Rule 7).
5. **Target resolution.** `target = LastInserted` uses the text MIA last injected this session (held
   in `DictationState`, [dictation.md](dictation.md)). `target = Selection` reads the user's current
   selection by issuing a clipboard-copy and reading it back (Rule 9), then **restores** the prior
   clipboard. If neither yields text → `Err("no target text")`, no-op.
6. **Language handling.** The *instruction* is transcribed and routed in its own language; the
   *content* keeps its language unless the action is `Translate` (then `params.lang` is the target).
   pt-BR and English have tailored prompts/few-shots; other languages are best-effort and the user is
   warned in Settings. Translate defaults its target to English when unspecified ("translate to
   English" is the canonical example), but honors an explicit target language.
7. **Ambiguous / unknown command → ask or no-op, never guess.** If the router is uncertain, or the
   parsed command fails validation, or the model's confidence/grammar match is poor, MIA does **not**
   transform: it surfaces `Err("command not understood")` to the HUD with a short hint, and the
   original text is left exactly as-is. A misheard command must never corrupt the user's text.
8. **Replace vs append; reversible.** Default delivery is **replace** the target (select the prior
   range, inject the new text — [text-injection.md](text-injection.md)). The original text is held so
   the user can **undo** the transform from the HUD/tray for a few seconds (Rule 10). Polish replaces
   in place the same way. Append mode is available for actions like Summarize via `params`.
9. **Selection capture is non-destructive.** Reading a selection uses a save → Ctrl+C → read →
   restore-clipboard sequence (the clipboard fallback machinery in
   [text-injection.md](text-injection.md)); the user's clipboard contents are always restored
   afterward, success or failure. If the focused app has no selection, the copy is empty → treat as
   "no target text" (Rule 5), no clipboard left dirty.
10. **Undo window.** After a transform is injected, MIA keeps the pre-transform text for a short
    window (default ~8 s) and shows an "Undo" affordance on the HUD; undo re-selects the new text and
    re-injects the original. After the window, the buffer is dropped. Undo is best-effort (depends on
    selection working in the target app) and never blocks dictation.
11. **Polish is faithful-leaning, not creative.** The Polish prompt is constrained to *repair* (fix
    disfluency, grammar, casing, light list/structure formatting, mild tone adaptation) and is
    instructed **not** to add facts, change meaning, or invent content. It is still an opt-in,
    reviewable action precisely because a 3B model can overstep (Section 7) — it is never auto-applied
    to dictation.
12. **Lazy load, idle unload, single in-flight.** The model loads on first `Command`/`Polish` and is
    kept for reuse; after a configurable idle timeout (default 5 min) it may be unloaded to reclaim
    ~1.5-2 GB RAM. Only **one** LLM request runs at a time; a second is rejected or queued (it never
    blocks the dictation hot path, which does not touch the LLM). `unload_llm` frees it immediately.
13. **Hard timeout and cancellation.** Every LLM request has a token/wall-clock cap; on timeout or
    user cancel it returns `Err("llm timeout")` / `Err("cancelled")` and leaves the text untouched.
    Generation runs on a worker thread, never on the cpal audio callback or the UI thread.
14. **Never on the default faithful path.** Plain dictation (`Intent::Dictation`) is guaranteed to
    reach injection via STT → deterministic cleanup with **no** LLM involvement — Rules 1, 2, and 12
    together ensure the LLM cannot insert itself into faithful output.

---

## 4. Options & Defaults

All user-facing options live in the Settings/Hub window ([settings.md](settings.md)) and flow into
`AiConfig` / `PolishOpts`. STT anti-hallucination flags are separate and **fixed** (ADR-007,
[speech-to-text.md](speech-to-text.md)).

| Option | Type | Range / values | Default | Effect |
|---|---|---|---|---|
| `ai_enabled` | bool | on/off | `false` | Master switch — when off, the router never runs and no model loads (Rule 1). |
| `model_id` | enum | `qwen2.5-3b-instruct-q4_k_m` \| `llama-3.2-3b-instruct-q4_k_m` | `qwen2.5-3b-instruct-q4_k_m` | Which GGUF to download/use. |
| `command_mode` | bool | on/off | `false` | Enable spoken Command Mode (requires `ai_enabled`). |
| `command_trigger` | enum | `prefix` \| `hotkey` \| `both` | `prefix` | How a command is signaled — "Hey MIA …" prefix, a dedicated hotkey ([hotkeys.md](hotkeys.md)), or both. |
| `trigger_phrase` | string | user text | `"Hey MIA"` | The prefix that routes an utterance to Command Mode (Rule 2). |
| `polish_enabled` | bool | on/off | `false` | Enable the explicit Polish action (button/hotkey/command). |
| `polish_tone` | enum | `neutral` \| `formal` \| `casual` | `neutral` | Tone for Polish (Rule 11). |
| `delivery_mode` | enum | `replace` \| `append` | `replace` | Default transform delivery (Rule 8). |
| `undo_window_secs` | int | 0-30 | `8` | How long the Undo affordance stays available (Rule 10); 0 disables undo. |
| `idle_unload_mins` | int | 0-60 | `5` | Idle minutes before the model unloads to free RAM (Rule 12); 0 = keep loaded. |
| `backend` | enum | `inprocess` \| `server` | `inprocess` | `llama-cpp-2` in-process (default) or `llama-server` sidecar fallback. |
| `max_tokens` | int | 64-1024 | `256` | Generation cap per request (Rule 13). |

Validation: the UI disables `command_mode`/`polish_enabled` until `ai_enabled` is on and the model is
installed; the engine re-checks (`ai_status`) defensively before every request. Trigger phrase is
trimmed/lowercased for matching. Low-RAM machines (Edge Cases) get a warning and may have AI gated.

---

## 5. Threading / Performance

Live dictation stays **latency-critical**; this tier must not bleak its cost onto the hot path.

- **Audio thread**: not involved in the transform. The cpal real-time callback only captures the
  *instruction* utterance (Command Mode) like any other dictation ([audio-capture.md](audio-capture.md));
  no LLM, injection, or clipboard work ever runs in it.
- **Warm model contract**: the **Whisper** model remains warm/resident
  ([ADR-004](architecture.md#adr-004-warmresident-stt-for-live-dictation)). The **LLM is
  deliberately not** kept warm by default — it is loaded lazily on first `Command`/`Polish` and
  idle-unloaded (Rule 12). This is the intentional asymmetry: STT must be sub-second every utterance;
  the LLM is an occasional, user-initiated detour. This module never cold-spawns `whisper-cli`.
- **Latency budget**: plain dictation is **unchanged** from Phase 1 — the only added cost on the
  default path is the sub-millisecond `route_intent` (pure, no model). When a command/polish *does*
  fire: model **load** on a cold start is seconds (a 1.5-2 GB GGUF mmap) — shown as a one-time
  "loading" state; once loaded, a short transform on a 3B Q4_K_M is dominated by **CPU prefill** of
  the target text + instruction (hundreds of ms to a few seconds depending on length and cores).
  This is acceptable because the user explicitly asked and is *not* mid-flow dictating.
- **Cancellation**: a hard token/wall-clock cap (`max_tokens`, timeout) plus a cancel flag in managed
  `State`; on cancel/timeout the partial generation is discarded and the text is left untouched
  (Rule 13). The dictation pipeline's own cancel ([dictation.md](dictation.md)) is independent.
- **Resource use**: the GGUF is ~1.5-2 GB on disk and a comparable ~1.5-2 GB RAM when loaded
  (Q4_K_M). It is **lazy-loaded** (Rule 12) and **idle-unloaded** to give the RAM back. CPU-only by
  default (no GPU requirement for the LLM in v1; CUDA acceleration is backlog, mirroring the optional
  Whisper CUDA engine in [speech-to-text.md](speech-to-text.md)). Single in-flight request (Rule 12).

---

## 6. UI States

Two surfaces: the **floating mic HUD** (dark, translucent, always-on-top —
[tray-and-hud.md](tray-and-hud.md)) for the live transform, and the **Settings/Hub window** (light
theme — [settings.md](settings.md), [design-system.md](design-system.md)) for enabling, model
management, and stats.

```
States (transform): Idle → [router: Command/Polish] → LoadingModel(spinner, first use only)
        → Thinking(spinner) → Applying(brief check) → Idle | Undo(8s) | Error(message)
Transitions: router=Command/Polish → LoadingModel (if cold) → Thinking; grammar-valid + applied
        → Applying → Undo window → Idle; ambiguous/parse-fail/timeout → Error(hint), text untouched.
Model lifecycle (Settings): NotInstalled → Downloading(%) → Installed(Unloaded) ⇄ Loaded.
```

- **HUD** (while transforming): a distinct **"thinking"** state (spinner) separate from STT's
  *Transcribing*, a one-time **"loading model"** state on a cold start, a brief **check** on apply,
  and an **Undo** affordance for `undo_window_secs` (Rule 10). Errors (Rule 7, ambiguous/timeout)
  show a short message; the text is never altered on error. Keep the single action-blue accent and
  the listening-pulse discipline; nothing of the LLM output renders in the HUD beyond status.
- **Settings/Hub** ("AI Magic" panel): master toggle, model picker + **download gate** with progress
  (Rule 3), Command Mode + trigger config, Polish toggle + tone, delivery/undo/idle options, the
  current `ai_status` (installed / loaded / RAM), an explicit **Unload model** button, and a clear
  honesty note that the LLM can overcorrect (Section 7) so it is opt-in and reviewable. Empty state
  when not installed; loading state during download; error state on failed download/load. Hit targets
  ≥40px; controls labelled, not color-only.

---

## 7. Edge Cases

| Scenario | Expected behavior |
|---|---|
| AI feature disabled (default) | Router never runs, no model loads; dictation is pure Phase 1 ([text-cleanup.md](text-cleanup.md)). Command/Polish actions return `Err("ai disabled")`. |
| Model not yet downloaded | `Err("model not installed")`; UI offers one-tap download (Rule 3). Dictation keeps working during download. |
| Low-RAM machine (< ~3 GB free) | `Err("insufficient memory: needs ~2 GB free")`; Settings warns and may gate the feature; dictation is unaffected (LLM is optional). |
| Ambiguous / misheard command | Router/parse uncertain → `Err("command not understood")` + HUD hint; **text left untouched** (Rule 7). Never guess or partially apply. |
| No target text (empty selection / nothing inserted yet) | `Err("no target text")`, no-op; clipboard restored (Rule 9). |
| Small-LLM overcorrection / hallucination | The reason this tier is **opt-in and off the faithful default path** (ADR-008): Polish is repair-constrained and reviewable (Rule 11); Command Mode is grammar-constrained (Rule 4); the Undo window (Rule 10) lets the user revert. Faithful dictation never routes through the LLM (Rule 14). |
| Focused window is elevated (UAC) | The replace/select keystrokes can't reach a higher-integrity window unless MIA is elevated ([ADR-005](architecture.md#adr-005-system-wide-text-injection-on-windows)) — surface it; the transform may compute but fail to apply. |
| Selection unavailable in target app (no Ctrl+C support) | Copy yields empty → treat as "no target text" (Rule 5); clipboard restored; no transform. |
| LLM load fails / corrupt GGUF | `Err("model load failed: …")`; offer re-download; dictation unaffected. |
| Generation exceeds timeout / user cancels | `Err("llm timeout")` / `Err("cancelled")`; partial output discarded, text untouched (Rule 13). |
| Translate without a target language | Defaults to English (Rule 6); honors an explicit "translate to <lang>". |
| Command fired while a transform is already running | Second request rejected/queued (single in-flight, Rule 12); never two concurrent LLM runs. |

---

## 8. Testing Checklist

- **Rust** (`cargo test`, no I/O — pure helpers only):
  - [x] `route_intent` — trigger-prefix match (pt-BR + en); imperative-command phrases → `Command`;
        explicit polish phrase → `Polish`; ordinary dictation (including imperative *content* like
        "tell him to call me") → `Dictation`; ambiguous → `Dictation` (conservative default).
  - [x] `command_grammar` — the GBNF grammar built + asserted to admit the `{action, target, params}`
        actions/targets. (Full admit/reject is enforced by llama.cpp at decode time.)
  - [x] `validate_parsed` — unknown action / missing required `params` (e.g. Translate without lang
        falls back, Custom without instruction rejected) → invalid; valid envelopes pass.
  - [x] `build_prompt` — per-action, per-language prompt contains the target text and instruction,
        with the faithful "don't invent facts" constraint. (The dedicated Polish prompt is runtime.)
  - [x] trigger-phrase tables present for PtBr/En; matching is trimmed/lowercased.
  - [ ] each `Err(String)` path is reachable from `run_command`/`polish`/`download_llm` (disabled,
        not-installed, no-target, not-understood, insufficient-memory, timeout, cancelled) — pending the runtime commands.
- **Manual / runtime** (needs mic, model, and a real focused app):
  - [ ] download gate: enable AI → download model with progress → status shows Installed/Loaded.
  - [ ] Command Mode: dictate text, then "make this more concise" / "more formal" / "turn into a
        bullet list" / "summarize" / "translate to English" → correct transform replaces the text
        (pt-BR and English).
  - [ ] Polish action over a disfluent dictation → cleaned without invented content; Undo reverts.
  - [ ] ambiguous/misheard command → HUD shows "command not understood", text untouched.
  - [ ] selection target: select text in an editor → command transforms the selection; clipboard
        restored afterward.
  - [ ] latency: plain dictation with AI enabled is indistinguishable from AI-disabled (router only).
  - [ ] idle-unload frees ~1.5-2 GB RAM; next command reloads with a one-time "loading" state.
  - [ ] elevated/UAC target window: transform computes but apply fails gracefully (surfaced).

## 9. Out of Scope (this version)

- **LLM on the default/faithful path or auto-Polish** — the default stays STT → deterministic cleanup
  → inject ([text-cleanup.md](text-cleanup.md), [dictation.md](dictation.md)). AI is always opt-in
  and user-initiated (ADR-008).
- **Always-warm / GPU-accelerated LLM, sub-second commands** — CUDA for the LLM and keeping it
  resident for instant commands are Phase 5 / Backlog ([../ROADMAP.md](../ROADMAP.md)); v1 is
  lazy-loaded CPU.
- **Larger / cloud models, RAG, multi-turn chat, agentic tool-use** — out of scope; MIA is a
  dictation app with a small *local* transformer, not a chatbot, and never calls a cloud (ADR-001).
- **Per-app / per-context writing styles and learned tone** — Phase 3 personalization
  ([../ROADMAP.md](../ROADMAP.md)); related: [custom-dictionary.md](custom-dictionary.md),
  [snippets.md](snippets.md).
- **Spoken-number-to-digit conversion and other rewriting that overlaps cleanup** — the deterministic
  tier stays non-rewriting ([text-cleanup.md](text-cleanup.md)); only the opt-in LLM rewrites, and
  only on request.
- **Languages beyond pt-BR/English with tuned prompts** — others are best-effort; tuned per-language
  Command/Polish prompts for more Whisper languages are backlog.
