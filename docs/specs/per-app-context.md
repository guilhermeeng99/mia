# Per-App Writing Styles / Context Feature Spec

> **Status**: Implemented (code-complete; on-device validation pending — see §8)
> **Last updated**: 2026-06-05
> **Coverage**: Sections 1–9 complete.
> **Environment**: desktop (Windows, native)

Per-app writing styles let MIA tailor a dictation to the **application the user is dictating
into**. At session start the orchestrator resolves the foreground window's executable name
(`win32::foreground_process_name`); at injection time it looks up the first matching rule and
applies its overrides — a fixed language, a clipboard-vs-`SendInput` backend, a trailing period,
or spoken-punctuation on/off. It sits at the **tail** of the pipeline (… → cleanup → dictionary →
snippets → **per-app style** → inject), is **off by default**, and never changes the transcript's
meaning — only formatting/targeting knobs already exposed elsewhere. Lands in **Phase 3 —
Personalization** ([../ROADMAP.md](../ROADMAP.md)); implements **ADR-005** (focus-aware injection)
and **ADR-006** (`Result<T, String>` IPC). Pairs with the focus/elevation probe used by
[text-injection.md](text-injection.md) Rules 6–7.

**Scope decisions** (locked at design time):

- **Match on the executable stem, not window title or UI Automation.** The foreground EXE name
  (`code`, `chrome`, `winword`) is cheap, stable, and privacy-safe; window-title/role matching
  would be brittle and read user content (ADR-001 / Phase 3).
- **Overrides only reuse knobs MIA already has** (language, inject backend, trailing period,
  spoken punctuation). No per-app LLM rewriting or custom grammars — MIA stays faithful and
  deterministic (the Phase-2 AI layer is descoped) (Phase 3).
- **Focus is captured at session *start*, applied at inject.** The user's hand is on the hotkey at
  start, so that EXE is the intended target even if focus drifts during transcription (ADR-005).
- **Off by default; safe-by-omission.** With `perApp.enabled = false` or no matching rule, the
  pipeline behaves exactly as before — every override defaults to "inherit" (Phase 3).
- **Best-effort, never fatal.** Any Win32 failure resolves to `None`/no-match and the global
  settings apply; a missing focus probe never blocks dictation (ADR-006).

---

## 1. Inputs / Outputs

| Aspect | This feature |
|---|---|
| **Trigger** | A normal dictation session (PTT) — no separate trigger. The lookup runs inside `start_dictation` (capture the EXE) and `stop_dictation` (apply the rule). |
| **Audio in** | N/A — operates on text + window metadata, not PCM. |
| **Text in** | The post-snippets text (and the chosen language feeds STT/cleanup earlier in the same call). |
| **Text out** | The same text, injected via the per-app backend; or an `Err` when the target is elevated (Rule 7). |
| **Target** | The OS-focused window (whose EXE was captured at start). |
| **Language** | The per-app override can pin `auto` or any supported dictation language option; otherwise it inherits the global dictation default. UI locale is unrelated. |

Crates: `windows-sys` (`GetForegroundWindow` / `GetWindowThreadProcessId` / `OpenProcess` /
`QueryFullProcessImageNameW` / token elevation) behind `win32.rs`. No audio, no disk, no network.

---

## 2. Engine Contract (Rust)

**Modules**: `app/src-tauri/src/app_styles.rs` (pure matching/merge), `app/src-tauri/src/win32.rs`
(focus probe), wired in `app/src-tauri/src/dictation.rs`; persisted in
`app/src-tauri/src/settings.rs` (the `perApp` group). No new command — the Hub edits the whole
`perApp` group through the existing `update_settings` (group-granular patch).

```rust
// settings::PerAppSettings (serde camelCase)
pub struct PerAppSettings { pub enabled: bool, pub styles: Vec<AppStyle> }

// app_styles::AppStyle — every override Option = "inherit the global setting"
pub struct AppStyle {
    pub match_exe: String,                 // case-insensitive substring of the EXE stem
    pub language: Option<DefaultLanguage>, // auto | pt | en | es | fr | ...
    pub inject_mode: Option<InjectMode>,   // auto | sendInput | clipboard
    pub ensure_trailing_period: Option<bool>,
    pub spoken_punctuation: Option<bool>,
}

// Pure helpers (cargo-tested, no I/O):
pub fn match_style<'a>(styles: &'a [AppStyle], exe: &str) -> Option<&'a AppStyle>; // longest-match wins
pub fn resolve_language(base: DefaultLanguage, style: Option<&AppStyle>) -> DefaultLanguage;
pub fn resolve_inject_mode(style: Option<&AppStyle>) -> InjectMode;               // None → Auto
pub fn merge_cleanup(base: CleanupOptions, style: Option<&AppStyle>) -> CleanupOptions;
pub fn sanitize(styles: Vec<AppStyle>) -> Vec<AppStyle>;                          // trim/drop-blank/dedup

// win32.rs (best-effort; Windows-only, stubs elsewhere):
pub fn foreground_process_name() -> Option<String>; // lowercased EXE stem
pub fn is_foreground_elevated() -> bool;            // target outranks MIA (UIPI)
pub fn has_foreground_window() -> bool;
```

- `start_dictation` stores `foreground_process_name()` in the managed `FocusContext`;
  `stop_dictation` consumes it and resolves the style; `cancel_dictation` clears it.
- `Err(String)` path: an elevated target returns
  `"janela em foco é elevada (UAC) — execute o MIA como administrador para digitar nela"` (Rule 7).
- The typed UI mirror lives in `app/src/lib/settings.ts` (`AppStyle`, `PerAppSettings`); the Hub's
  `PerAppSection.svelte` PATCHes the `perApp` group. The UI holds no matching logic.

---

## 3. Business Rules

1. **Disabled = no-op.** With `perApp.enabled = false`, no lookup runs; the global language,
   cleanup, and `Auto` injection apply unchanged.
2. **Match = case-insensitive substring of the EXE stem; longest `match_exe` wins.** `visualstudio`
   beats `studio`; a blank/whitespace rule never matches (`match_style`).
3. **No match → global defaults.** A matched rule with all-`None` overrides is equivalent to no
   match (every field inherits).
4. **Language override applies before STT.** The resolved language feeds both the `/inference`
   `language=` and the cleanup language for the same utterance (so the override is faithful, not a
   post-hoc relabel).
5. **Inject-mode override** replaces the `Auto` backend decision for the matched app (e.g. force
   `clipboard` for an app that mishandles synthetic keystrokes).
6. **Cleanup overrides are field-granular.** Only `ensureTrailingPeriod` / `spokenPunctuation` that
   the rule sets are changed; the rest inherit the global cleanup toggles (`merge_cleanup`).
7. **Focus captured at start, applied at inject.** If the probe fails at start, the session simply
   has no per-app context (global defaults).
8. **Elevated target → clear error (Rule 7).** When the foreground window outranks MIA, injection
   would be silently dropped by UIPI, so MIA returns the run-as-administrator error instead.
9. **No detectable foreground window → clipboard (Rule 6, best-effort).** Rather than synthesize
   keystrokes into the void, MIA falls back to the clipboard backend.
10. **Persistence is defensive.** `settings::validate` runs `sanitize` (trim, drop blank, dedup by
    EXE) so a malformed UI write can't break matching.

---

## 4. Options & Defaults

| Option | Type | Range / values | Default | Effect |
|---|---|---|---|---|
| `perApp.enabled` | bool | on / off | `false` | Master gate for the whole feature. |
| `style.matchExe` | string | EXE substring | — | Which app the rule targets (e.g. `code`). |
| `style.language` | enum? | `auto` plus supported dictation language codes · *inherit* | *inherit* | Pin the dictation language for this app. |
| `style.injectMode` | enum? | `auto` · `sendInput` · `clipboard` · *inherit* | *inherit* | Injection backend for this app. |
| `style.ensureTrailingPeriod` | bool? | on / off / *inherit* | *inherit* | Force/forbid a trailing period. |
| `style.spokenPunctuation` | bool? | on / off / *inherit* | *inherit* | Enable/disable spoken-punctuation substitution. |

`*inherit*` is encoded as the field being absent/`null`. Anti-hallucination STT defaults remain
fixed (ADR-007) and are **not** per-app tunable.

---

## 5. Threading / Performance

- **No audio-thread work.** The Win32 probes run on the command thread (`start`/`stop_dictation`),
  not the cpal callback. `foreground_process_name` is a couple of cheap syscalls.
- **No model work.** Language selection only changes the `/inference` argument; it never restarts
  the warm engine (ADR-004) — the same warm server handles any language.
- **Latency budget**: negligible (microseconds of Win32 calls); entirely off the STT hot path.
- **Cancellation**: `cancel_dictation` clears the `FocusContext`; nothing is injected.
- **Resource use**: none beyond the small `Vec<AppStyle>` in settings.

---

## 6. UI States

This feature has **no HUD state** — it's invisible at runtime (it only changes which language /
backend / cleanup the existing states use). Its controls live in the **Settings/Hub** window:

- **`PerAppSection.svelte`** (light theme): a master "Ativado" toggle, the list of rules (EXE →
  human-readable summary, with Remove), and an add form (EXE + language/inject/trailing-period
  selects, each defaulting to "Herdar").
- Empty state: "Nenhuma regra ainda." Errors surface inline above the list.

---

## 7. Edge Cases

| Scenario | Expected behavior |
|---|---|
| `perApp.enabled = false` | No lookup; global behavior (Rule 1). |
| Foreground EXE not resolvable | No per-app context; global defaults (Rule 7). |
| Two rules match | Longest `match_exe` wins (Rule 2). |
| Rule with all-`None` overrides | Equivalent to no match (Rule 3). |
| Focused window is elevated (UAC), MIA not | `Err(...)` run-as-administrator message; nothing injected (Rule 8). |
| No foreground window at inject | Clipboard backend fallback (Rule 9). |
| Blank/duplicate `matchExe` saved | Dropped/dedup'd by `sanitize` on persist (Rule 10). |
| Non-Windows build | `win32` stubs return `None`/`false`/`true`; feature is inert. |

---

## 8. Testing Checklist

- **Rust** (`cargo test`, pure helpers only):
  - [x] `match_style` — case-insensitive substring, longest-match-wins, blank never matches.
  - [x] `resolve_language` / `resolve_inject_mode` — override vs inherit vs `Auto`.
  - [x] `merge_cleanup` — only set overrides applied; unset inherited.
  - [x] `sanitize` — trims, drops blank, dedups by EXE.
  - [x] `settings::validate` sanitizes the `perApp.styles` vector.
- **Manual / runtime** (needs a real desktop + multiple apps; owner-validated):
  - [ ] A rule for one app (e.g. force clipboard, pin Spanish) applies only in that app.
  - [ ] Elevated/UAC window yields the run-as-administrator error, non-elevated injects normally.
  - [ ] Language override changes recognition + cleanup for the matched app.
  - [ ] Disabling the master toggle restores global behavior everywhere.

---

## 9. Out of Scope (this version)

- **Window-title / control-role / UI-Automation matching** — brittle and reads user content; EXE
  matching only (revisit only with a privacy-safe, reliable signal).
- **Per-app LLM rewriting / tone presets** — depends on the descoped Phase-2 AI layer
  ([../ROADMAP.md](../ROADMAP.md)).
- **Per-app hotkeys or per-app dictionaries/snippets** — the dictionary/snippets are global in V1.
- **True editable-target detection (Rule 6 full)** — only a foreground-window-exists proxy is done;
  reliable editable detection needs UI Automation, deferred ([text-injection.md](text-injection.md)).
