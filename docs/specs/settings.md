# Settings & "The Hub" Feature Spec

> **Status**: Phase 1 — `settings.rs` persistence implemented: the full `Settings` tree (§4) with per-group defaults + `schemaVersion`, the pure `apply_patch` / `validate` / `migrate` / `parse_settings` core (cargo-tested), failure-safe `load_settings` (missing → defaults; corrupt → defaults + sidelined backup), atomic `save_settings`, and the `get/update/reset_settings` commands wired into the handler + a managed `SettingsState` loaded at startup. Typed `settings.ts` wrapper. `update_settings` re-registers the PTT hotkey when it changes (Rule 8, before persisting), invalidates the warm engine on a model change, and syncs launch-at-login. The mic test now streams a **live level meter** (`test_microphone` takes a `Channel<CaptureEvent>`; the Hub + onboarding render a live bar). New group: the `perApp` group (per-app writing styles — see [per-app-context.md](per-app-context.md)). The signed updater is wired (ADR-009).
> **Last updated**: 2026-05-30
> **Coverage**: Sections 1-9 drafted. Phase 2 (AI tab) and Phase 4 (auto-update, stats) sections are forward-looking.
> **Environment**: desktop (Windows, native)

The **Settings window** is MIA's one and only real window — the [Blush Playground](design-system.md)
surface that the user opens from the system tray. It is "The Hub": a sidebar-navigated dashboard
that hosts every user-facing control (general behavior, hotkey, model/engine, audio device,
cleanup rules, dictionary, snippets, AI, about/updates) plus a small **local-only usage stats**
panel (words dictated, words-per-minute, daily streak). Like every MIA window it is a **thin
Svelte webview** ([architecture.md](architecture.md)): it renders state and calls typed
`invoke()` wrappers; it holds **no** dictation logic. All persisted preferences live in a single
JSON file in the app-data dir, read and written through the Rust **settings** command group. The
Hub lands across Phase 1 (its core panels) and Phase 4 (the polished "Hub" dashboard, stats, and
auto-update UI — see [../ROADMAP.md](../ROADMAP.md)); it implements
[ADR-002](architecture.md#adr-002-tauri-2-rust-engine--svelte-5--vite--tailwind-v4-ui),
[ADR-006](architecture.md#adr-006-resultt-string-error-model-across-the-rust--ui-ipc),
[ADR-009](architecture.md#adr-009-distribution--signed-in-app-auto-update), and surfaces the
permission-honesty of [ADR-001](architecture.md#adr-001-native-on-device-privacy-first) /
[ADR-005](architecture.md#adr-005-system-wide-text-injection-on-windows).

**Scope decisions** (locked at design time):

- **One window, sidebar-navigated.** A single resizable light window with a left sidebar of
  sections (no multi-window settings, no native OS preferences panel). Keeps the surface count to
  two (Hub + HUD) per the [design system](design-system.md#scope-decisions-locked). (ADR-002 / Phase 1).
- **Settings persist as one JSON file in app-data.** A single `settings.json` is the source of
  truth — human-readable, versioned, easy to back up, and trivially defaulted when missing.
  No registry keys, no scattered files. (ADR-006 / Phase 1).
- **Stats are local-only and on by default-but-disable-able.** Usage stats (word counts, WPM,
  streak) are computed and stored **only on the user's machine** — never uploaded, there is no
  telemetry of any kind. A toggle disables and clears them. (ADR-001 / Phase 4).
- **The Hub holds no dictation logic.** Every control is a thin wrapper over a Rust command;
  changing a model, device, or hotkey is the engine's job, the UI only invokes and renders.
  (ADR-002 / Phase 1).
- **Failure-safe load.** A corrupt or missing `settings.json` never blocks startup — the engine
  falls back to typed defaults and (for corruption) sidelines the bad file rather than refusing
  to run. (ADR-006 / Phase 1).
- **AI tab gated to Phase 2.** The AI section renders but its controls are disabled / "coming
  soon" until the local-LLM feature ships ([ai-commands.md](ai-commands.md)). (ADR-008 / Phase 2).

---

## 1. Inputs / Outputs

The Hub is the app's **settings/stats surface**, not part of the live audio→text hot path. It
reads and writes preferences and reads accumulated stats; it never captures audio or injects text
itself.

| Aspect | This feature |
|---|---|
| **Trigger** | Tray "Settings" / "Open MIA" action, or the app's first run ([onboarding.md](onboarding.md)) opens the Hub; in-window navigation between sections. |
| **Audio in** | N/A for the Hub itself, **except** the Audio tab's **mic level test**, which subscribes to a transient cpal level-meter stream ([audio-capture.md](audio-capture.md)) only while that tab is open. |
| **Text in** | N/A (dictionary/snippet text entry is delegated to [custom-dictionary.md](custom-dictionary.md) / [snippets.md](snippets.md)). |
| **Text out** | Persisted settings (`settings.json`) and accumulated stats (`stats.json`); no text injected at the cursor. |
| **Target** | The Hub window (light theme); writes go to the app-data dir on disk. |
| **Language** | UI is English (V1); the **default dictation language** setting (`auto`/`pt`/`en`) is one of the values managed here and consumed by [speech-to-text.md](speech-to-text.md). |

Backed by: `tauri-plugin-store` or a hand-rolled serde-JSON store in `settings.rs` for the
preferences file; `tauri-plugin-updater` for the About/Updates tab (ADR-009); the existing model
registry / download / CUDA-detect machinery in `stt.rs` for the Model tab
([speech-to-text.md](speech-to-text.md)); cpal for the Audio tab's level meter. **No audio buffer
ever touches disk** (ADR-001) — the level test reads RMS amplitude in memory only.

---

## 2. Engine Contract (Rust)

Rust is the **engine**; the Hub UI calls typed `invoke()` wrappers (one group per Rust module,
exposed from `app/src/lib/settings.ts` and `app/src/lib/stats.ts`). All commands return
`Result<T, String>` (ADR-006).

**Modules**: `app/src-tauri/src/settings.rs` (preferences + persistence),
`app/src-tauri/src/stats.rs` (usage stats), with the Model/Audio/Update tabs delegating to
`stt.rs`, the `audio.rs` device helper, and the updater plugin respectively.

```rust
// ---- settings.rs ----

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Result<Settings, String>;
// Returns the in-memory Settings (loaded once at startup; defaults if file missing/corrupt).

#[tauri::command]
fn update_settings(state: State<'_, AppState>, patch: SettingsPatch) -> Result<Settings, String>;
// Merge-patch: only the provided fields change. Validates, persists to settings.json
// (atomic write: temp file + rename), applies side effects (re-register hotkey, set
// launch-at-login, swap warm model/engine), returns the full new Settings.

#[tauri::command]
fn reset_settings(state: State<'_, AppState>) -> Result<Settings, String>;
// Overwrite with defaults; persist; re-apply side effects. Used by "Reset to defaults".

#[tauri::command]
fn list_input_devices() -> Result<Vec<AudioDevice>, String>;             // Audio tab picker — IMPLEMENTED (audio.rs)

// --- mic test (IMPLEMENTED, audio.rs) — streams a live RMS meter to the Hub + onboarding ---
#[tauri::command]
fn test_microphone(state: State<'_, CaptureState>, ms: Option<u32>, level: tauri::ipc::Channel<CaptureEvent>) -> Result<MicTest, String>;

// Launch-at-login is NOT a standalone command: toggling `general.launchAtLogin` via
// `update_settings` syncs tauri-plugin-autostart as a side effect (re-applied at startup).

// ---- stats.rs (IMPLEMENTED) ----

#[tauri::command]
fn get_stats(state: State<'_, AppState>) -> Result<UsageStatsView, String>;  // Hub dashboard (derived totals + WPM + streak)
#[tauri::command]
fn reset_stats(state: State<'_, AppState>) -> Result<(), String>;        // clear local stats

// ---- About / Updates (delegates to tauri-plugin-updater, ADR-009) — Phase-pending ----

#[tauri::command]
async fn check_for_update() -> Result<UpdateInfo, String>;               // {available, version, notes} — Phase-pending
#[tauri::command]
async fn install_update(channel: tauri::ipc::Channel<Progress>) -> Result<(), String>; // Phase-pending
```

> **Implementation status.** Registered today: `list_input_devices` (in `audio.rs`), the
> `settings.rs` group (`get_settings` / `update_settings` / `reset_settings`), the `stats.rs`
> group (`get_stats` / `reset_stats`), and the mic test `test_microphone` (audio.rs). Launch-at-login
> is **not** a command — it is an `update_settings` side effect (toggling `general.launchAtLogin`
> syncs `tauri-plugin-autostart`). The in-app updater is wired (Phase 4, ADR-009) via the
> `tauri-plugin-updater` JS API (`check`/`downloadAndInstall`/`relaunch`, see `update.ts`), not via
> the `check_for_update` / `install_update` custom commands sketched above.

- **`Settings`** (serde `rename_all = "camelCase"`) — the full preferences tree, see §4. Carries a
  `schemaVersion: u32` for forward migration.
- **`SettingsPatch`** — same shape with every field `Option<T>`; merge semantics so the UI can
  PATCH a single toggle without round-tripping the whole tree.
- **`AudioDevice`** `{ id: String, name: String, isDefault: bool }`; `UsageStats` see §1/§6;
  `UpdateInfo` `{ available: bool, version: Option<String>, notes: Option<String> }`.
- **Side effects** of `update_settings` happen **inside** the command, atomically with the write:
  rebinding the PTT hotkey ([hotkeys.md](hotkeys.md)), toggling launch-at-login (Windows
  Run-key / Startup), and a **warm-model swap** when the selected model or engine changes
  ([ADR-004](architecture.md#adr-004-warmresident-stt-for-live-dictation)). If a side effect fails
  (e.g. the new hotkey is already taken), the command returns `Err(..)` and **does not** persist
  the offending change.
- **`Err(String)` cases** map 1:1 to UI errors: `"settings file is read-only or locked"`,
  `"hotkey already in use: <combo>"`, `"no input device named <id>"`, `"could not enable launch
  at login"`, `"update check failed (offline)"`, `"model not downloaded"`.
- **Pure helpers** (`#[cfg(test)]`): `Settings::default()`, `apply_patch(&Settings, &SettingsPatch)`,
  `migrate(json, from_version)`, `validate(&Settings)` (clamps/normalizes — e.g. unknown language →
  `auto`), WPM/streak arithmetic in `stats.rs`. None do I/O.
- The model/engine controls reuse the **`stt.rs`** command surface and the
  `ModelDownloadGate` / progress-`Channel` UX from [speech-to-text.md](speech-to-text.md) — the Hub
  does not re-implement download logic.

---

## 3. Business Rules

1. **Single source of truth.** All preferences live in one `settings.json` under the OS app-config
   dir (`%APPDATA%\com.mia.app\settings.json` via Tauri's `app_config_dir`). Stats live in a
   separate `stats.json` in the app-data dir so wiping stats never touches preferences.
2. **Load once, serve from memory.** Settings are read into a managed `State` at startup;
   `get_settings` returns that in-memory copy. The file is only re-read after an external change is
   not assumed — the in-memory copy is authoritative for the process lifetime.
3. **Patch, validate, persist atomically.** `update_settings` merges the patch, runs `validate`
   (clamping ranges, normalizing enums), writes to a temp file and **renames** over `settings.json`
   (crash-safe), then applies side effects. A write failure returns `Err` and leaves the prior file
   intact; the in-memory copy is rolled back to the last persisted state.
4. **Missing file → defaults, written on first save.** If `settings.json` does not exist, the
   engine uses `Settings::default()` and creates the file on the first `update_settings` (or
   immediately after onboarding). First run is never an error.
5. **Corrupt file → defaults + sidelined backup.** If `settings.json` exists but fails to parse,
   the engine logs a warning, renames the bad file to `settings.corrupt-<timestamp>.json`, and
   continues with defaults. The Hub surfaces a non-blocking notice ("Your settings were reset
   because the file was unreadable; a backup was kept"). The app **never** refuses to start over a
   bad settings file (ADR-006).
6. **Schema migration.** On load, if `schemaVersion` is older than current, `migrate` upgrades the
   tree in memory and the next save writes the new version. Unknown future fields are preserved
   where possible, or dropped with a logged warning if the version is newer than the running app.
7. **Default language is `auto`.** The General tab's default dictation language is one of
   `auto` | `pt` | `en`; `auto` lets Whisper detect ([speech-to-text.md](speech-to-text.md)).
   pt-BR and English are first-class; `auto` is the default.
8. **Hotkey rebind is validated by the engine.** Saving a new PTT chord re-registers it via the
   `tauri-plugin-global-shortcut` plugin inside `update_settings`; a conflict (already-registered combo, or a
   reserved system chord) returns `Err("hotkey already in use: …")` and the old binding stays
   active ([hotkeys.md](hotkeys.md)). Mode (`hold` push-to-talk vs `toggle`) is a separate setting.
9. **Model selection gates on download.** Picking a model that isn't present surfaces the
   `ModelDownloadGate` rather than silently switching; the engine refuses to set a model that has
   no local file (`Err("model not downloaded")`). Switching models or CPU↔CUDA triggers a warm-model
   **swap** with a brief "loading model" state ([ADR-004](architecture.md#adr-004-warmresident-stt-for-live-dictation)).
10. **GPU status is detected, never assumed.** The Model tab shows CUDA availability from
    `nvcuda.dll` detection (reused from Toolzy); if no NVIDIA GPU is present the CUDA-engine row is
    shown as unavailable, not offered for download.
11. **Audio device picker lists live devices; default is "System default".** `list_input_devices`
    enumerates cpal input devices; the stored value may be a device id or the sentinel
    `"default"`. If a previously-selected device is gone at startup, the engine falls back to the
    system default and notes it (no hard error).
12. **Mic test is transient and HUD-independent.** `start_mic_test` opens a temporary capture that
    streams RMS level to the Hub's level meter; it is automatically stopped on tab change / window
    close / `stop_mic_test`, and never writes audio to disk (ADR-001).
13. **Cleanup toggles are individual and default-on.** Each deterministic cleanup rule group
    (filler removal, spoken-punctuation, stutter collapse, capitalization) is independently
    toggleable; all default **on**. Toggling them only affects future dictations
    ([text-cleanup.md](text-cleanup.md)).
14. **AI controls are inert until Phase 2.** The AI tab renders disabled with a "Phase 2" note;
    enabling local-LLM Command Mode / Polish and picking the LLM model becomes active only when
    [ai-commands.md](ai-commands.md) ships. Until then these settings have no engine effect.
15. **Stats accumulate locally and are disable-able.** Each successful dictation increments local
    counters (words, time, day-streak); `get_stats` reads them; a General/Stats toggle stops
    collection and `reset_stats` clears `stats.json`. No stat ever leaves the machine (ADR-001).
16. **Update check is non-throwing.** `check_for_update` against GitHub Releases (ADR-009) returns
    `{available:false}` on offline/error rather than surfacing a scary error; `install_update`
    streams progress and verifies the minisign signature before applying.
17. **Launch-at-login is a real Windows integration.** The General toggle writes/removes a
    Run-key (or Startup entry) via `set_launch_at_login`; a failure (permissions) returns `Err` and
    the toggle reverts in the UI.

---

## 4. Options & Defaults

Every user-facing preference and its default. Anti-hallucination STT defaults (Silero VAD +
greedy/temperature-0 decoding with no temperature fallback, and stateless per-utterance
recognition so there is no cross-utterance context drift — see [speech-to-text.md](speech-to-text.md))
are **fixed and not exposed here** (ADR-007) — the Hub never offers a control that would weaken
transcription fidelity.

### General
| Option | Type | Range / values | Default | Effect |
|---|---|---|---|---|
| `launchAtLogin` | bool | — | `false` | Start MIA (to tray) on Windows login. Writes a Run-key. |
| `defaultLanguage` | enum | `auto` \| `pt` \| `en` | `auto` | Forces Whisper language or auto-detect ([speech-to-text.md](speech-to-text.md)). |
| `playSounds` | bool | — | `false` | Optional start/stop chime (off = unobtrusive default, design §9a). |
| `collectStats` | bool | — | `true` | Enable local usage-stat collection (§6). |

### Hotkey
| Option | Type | Range / values | Default | Effect |
|---|---|---|---|---|
| `hotkey` | string (chord) | any valid `tauri-plugin-global-shortcut` combo | `Ctrl+Space` (locked default — `DEFAULT_ACCEL` in `hotkey.rs`; see [hotkeys.md](hotkeys.md)) | The push-to-talk binding. |
| `hotkeyMode` | enum | `hold` \| `toggle` | `hold` | `hold` = record while held; `toggle` = press to start, press to stop. |

### Model / Engine
| Option | Type | Range / values | Default | Effect |
|---|---|---|---|---|
| `model` | enum | entries from the `MODELS` registry (`small`, `medium`, `large-v3-turbo`, `large-v3` — there is no `base`) | `small` (CPU) | Which Whisper model the warm engine loads. Gates on download (rule 9). |
| `engine` | enum | `cpu` \| `cuda` | `cpu` | Inference backend; `cuda` only selectable when detected + downloaded (rule 10). Swaps the warm model. |
| `unloadOnIdle` | bool | — | `true` | Evict the warm model after idle to free RAM ([ADR-004](architecture.md#adr-004-warmresident-stt-for-live-dictation)). |

### Audio
| Option | Type | Range / values | Default | Effect |
|---|---|---|---|---|
| `inputDevice` | string | `"default"` or a cpal device id | `"default"` | Mic used for capture; falls back to system default if missing (rule 11). |

### Cleanup
| Option | Type | Range / values | Default | Effect |
|---|---|---|---|---|
| `cleanup.fillerRemoval` | bool | — | `true` | Strip filler words (um/uh/é/tipo/né). |
| `cleanup.spokenPunctuation` | bool | — | `true` | "nova linha"/"ponto"/"vírgula" → punctuation/newlines. |
| `cleanup.stutterCollapse` | bool | — | `true` | Collapse stutters/repeats. |
| `cleanup.capitalization` | bool | — | `true` | Sentence-case / capitalization fixer. |

(All cleanup rules detailed in [text-cleanup.md](text-cleanup.md).)

### HUD / Recording indicator
| Option | Type | Range / values | Default | Effect |
|---|---|---|---|---|
| `indicator` | enum | `overlay` \| `tray` \| `both` | `both` | Which recording indicator(s) show the live phase during dictation: the floating HUD overlay, a colored badge on the tray icon, or both. Set in the **Ditado** view; read by `dictation.rs::show_phase` ([tray-and-hud.md](tray-and-hud.md)). |
| `hudPosition` | enum | `caret` \| `bottom-center` \| `bottom-right` | `caret` | Where the floating HUD anchors ([design-system.md](design-system.md#8b-floating-mic-hud-dark-frameless-always-on-top), [tray-and-hud.md](tray-and-hud.md)). |

### AI (Phase 2 — inert until [ai-commands.md](ai-commands.md))
| Option | Type | Range / values | Default | Effect |
|---|---|---|---|---|
| `ai.enabled` | bool | — | `false` | Master switch for the local LLM (Command Mode + Polish). |
| `ai.model` | enum | `qwen2.5-3b` \| `llama-3.2-3b` (Q4_K_M) | `qwen2.5-3b` | Which llama.cpp model to load when AI is on. |
| `ai.polishOnInsert` | bool | — | `false` | Opt-in "Polish" rewrite before injection. |

### Updates
| Option | Type | Range / values | Default | Effect |
|---|---|---|---|---|
| `autoCheckUpdates` | bool | — | `true` | Check GitHub Releases for a signed update at startup (ADR-009). |

Validation: the UI **disables** invalid choices (e.g. `cuda` when no GPU, a model with no local
file beyond the download gate, the AI tab pre-Phase-2); the engine **re-checks** every value in
`validate`/side-effects and returns `Err` defensively (rule 3) — the UI is never trusted to be the
only guard.

---

## 5. Threading / Performance

The Hub is **not** on the dictation hot path, so the warm-model contract is observed but not
exercised here except via deliberate model/engine swaps.

- **No audio-thread involvement** except the Audio tab's mic test, which spins a short-lived cpal
  capture on its own thread and streams RMS level over a Tauri `Channel<f32>`; it never blocks the
  UI and is torn down on tab/window close (rule 12). It does **not** run the warm STT and never
  writes to disk.
- **Settings I/O is cheap and off the UI thread.** `update_settings` does a small atomic file
  write; commands run off the WebView thread. The only potentially slow side effect is a
  **warm-model swap** (model/engine change) — that reuses the load path with a Tauri progress
  `Channel` and a "loading model" UI state ([ADR-004](architecture.md#adr-004-warmresident-stt-for-live-dictation));
  the Hub does not cold-spawn `whisper-cli`.
- **Model download** (Model tab) runs the existing `stt.rs` HF download on a
  worker with streamed progress and cancel-via-`State`, reusing Toolzy's pattern — not blocking
  the window.
- **Stats writes are debounced.** Per-dictation counters update the in-memory `UsageStats`
  immediately; `stats.json` is flushed lazily (on a timer / on window close / on app exit) so
  frequent dictation doesn't hammer the disk.
- **Resource use.** The Hub itself is light (a WebView). The expensive resident resource remains
  the warm STT model (and, in Phase 2, the optional LLM); the `unloadOnIdle` setting governs its
  eviction.

---

## 6. UI States

The Hub is a **Blush Playground window** ([design-system.md](design-system.md#8a-settings--hub-window-light));
it owns no HUD state (the floating mic HUD is a white Blush pill — white, 2px charcoal outline —
sharing the one Blush language, see [tray-and-hud.md](tray-and-hud.md)). Its states are
per-section data states plus a global load/error.

```
Window: Loading(settings fetch) → Ready(sections interactive) → SavingPatch(optimistic) → Ready
                                 → Error(load failed → defaults + notice banner)
Mic test (Audio tab): Off → Testing(live level meter) → Off  (auto-stop on leave)
Model swap: Selected → LoadingModel(progress) → Active | Error(revert)
Update (About tab): Idle → Checking → UpToDate | UpdateAvailable → Downloading(progress) → Restart
```

- **Layout** — left **sidebar** of views (Visão geral, Ditado, Modelos & Motor, Dicionário,
  Snippets, Por app) + a content area, one `Card` per logical group. Hotkey, audio device, and
  default language live **inside the "Ditado" view** (there is no separate Hotkey/Audio/Cleanup/AI
  tab). A **header strip** shows the logo, a `native` "100% local · offline" badge, and a
  right-aligned version / "Update to vX" button. Active nav item = **pumpkin fill** (the active
  `NavItem` is `border-charcoal bg-pumpkin`); the action-color discipline holds (design §2, §8a).
- **The Hub dashboard** (top of General, or its own "Stats" panel): three local-only stat cards —
  **words dictated** (total + this week), **average words-per-minute**, and **day streak** — each a
  `StatTile` with its accent fill, charcoal figures and ink-soft labels. Empty state before
  any dictation: a friendly "Start dictating to see your stats" placeholder. A small "Reset stats"
  ghost/danger action and the `collectStats` toggle live here.
- **Controls** use the shared `ui/` components: `Toggle` (launch-at-login, cleanup rules, AI,
  auto-update — always with on/off text, never color-only), `HotkeyRecorder` (PTT rebind),
  `Field`/select (language, model, engine, device, HUD position), `ModelDownloadGate` (model not
  present), and progress bars for download/update.
- **Empty / loading / error per section**: device list loading spinner; model row "downloading…"
  progress; update "Checking…"; corrupt-settings notice banner (rule 5). Errors render inline in
  `danger`, paired with text (accessibility §9c of the design system).
- Keyboard-first and accessible: visible `focus-visible:ring-4 ring-pumpkin/45` focus rings, hit
  targets ≥ 40px, no color-only states (design §9c).

---

## 7. Edge Cases

| Scenario | Expected behavior |
|---|---|
| `settings.json` missing (first run) | Use `Settings::default()`; create the file on first save. Not an error (rule 4). |
| `settings.json` corrupt / unparseable | Sideline as `settings.corrupt-<ts>.json`, continue with defaults, show a non-blocking Hub notice (rule 5). App never fails to start. |
| Settings file read-only / locked | `update_settings` → `Err("settings file is read-only or locked")`; in-memory copy rolled back; UI shows the error, prior file intact (rule 3). |
| Older `schemaVersion` | `migrate` upgrades in memory; next save writes the new version (rule 6). |
| Newer `schemaVersion` (downgrade) | Load best-effort, preserve unknown fields where possible, warn; never crash (rule 6). |
| New hotkey already in use / reserved | `Err("hotkey already in use: <combo>")`; old binding stays; UI shows inline error on the recorder (rule 8). |
| Selected model not downloaded | Surface `ModelDownloadGate`; engine refuses to set it (`Err("model not downloaded")`) (rule 9). |
| `cuda` chosen with no NVIDIA GPU | Option disabled/unavailable in UI; engine rejects defensively; stays on `cpu` (rule 10). |
| Stored input device no longer present | Fall back to system default at startup, note it; no hard error (rule 11). |
| Mic test started but tab/window closed | `stop_mic_test` auto-fires; capture torn down; no orphaned stream, nothing written to disk (rule 12). |
| Launch-at-login write fails (permissions) | `Err("could not enable launch at login")`; UI toggle reverts (rule 17). |
| Update check while offline | `{available:false}` returned, no error surfaced (rule 16, ADR-009). |
| Update signature verification fails | `install_update` returns `Err`; the tampered artifact is rejected, current version untouched (ADR-009). |
| Stats disabled mid-use | Collection stops immediately; existing `stats.json` kept until `reset_stats` clears it (rule 15). |
| Model/engine swap fails to load | Revert to the previously-active model; show error; dictation keeps working on the old model (rule 9, ADR-004). |

---

## 8. Testing Checklist

- **Rust** (`cargo test`, pure helpers, no I/O):
  - [x] `Settings::default()` produces the documented defaults (§4).
  - [x] `apply_patch` merges only provided fields; absent fields unchanged.
  - [x] `validate` clamps/normalizes (unknown language → `auto`, invalid enum rejected).
  - [x] `migrate(json, from_version)` upgrades old shapes; preserves/handles unknown fields (v1 scope: missing `schemaVersion` + tolerant partial-group load).
  - [x] WPM and streak arithmetic in `stats.rs` (boundary days, zero-dictation, streak break/resume).
  - [ ] each `Err(String)` path string is the documented message — pending the `update_settings` side effects.
- **Manual / runtime** (real Windows host, mic, model):
  - [ ] missing `settings.json` → defaults, file created on first save.
  - [ ] corrupt `settings.json` → sidelined backup + defaults + Hub notice; app starts.
  - [ ] toggle launch-at-login → Run-key created/removed; survives reboot.
  - [ ] rebind PTT to a free combo (works) and to a taken combo (rejected, old kept).
  - [ ] switch `hold`↔`toggle` mode and confirm dictation behavior ([hotkeys.md](hotkeys.md)).
  - [ ] pick an undownloaded model → download gate; CPU↔CUDA swap shows "loading model".
  - [ ] device picker lists mics; mic-level test animates and stops on tab change.
  - [ ] toggling each cleanup rule changes only subsequent dictations ([text-cleanup.md](text-cleanup.md)).
  - [ ] Hub stat cards increment after real dictations; "Reset stats" clears them; disable stops collection.
  - [ ] "Check for update" online (finds/none) and offline (no scary error); signed install path.
  - [ ] full round-trip: change several settings, restart app, values persist.

---

## 9. Out of Scope (this version)

- **Dictionary / Snippet editors** — the Hub only *links* to those sections; their data models and
  editors live in [custom-dictionary.md](custom-dictionary.md) and [snippets.md](snippets.md)
  (Phase 3).
- **Cloud sync / account / settings backup-to-server** — never (privacy-first, ADR-001). Settings
  are a local file the user can copy themselves.
- **Telemetry / analytics** — none, ever. Stats are local-only and disable-able (ADR-001).
- **A separate dark theme** — there is one Blush Playground language across both surfaces; the
  mic HUD is a white Blush pill, not a dark surface
  ([design-system.md](design-system.md#11-out-of-scope-v1)).
- **Exposing anti-hallucination STT internals** as tunable settings — fixed by ADR-007.
- **macOS / Linux settings parity** — Windows-only V1 (ADR-011); Phase 5 / Backlog.
