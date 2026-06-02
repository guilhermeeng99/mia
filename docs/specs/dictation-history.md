# Dictation History Feature Spec

> **Status**: Implemented
> **Last updated**: 2026-06-02
> **Coverage**: Sections 1-9 complete
> **Environment**: desktop (Windows, native)

Local history of non-empty dictation results. It sits after STT, cleanup, custom dictionary, and
snippet expansion, and before text injection, so users can recover a phrase even if the focused
target rejects insertion. Text stays on the user's machine in `history.json` (ADR-001).

**Scope decisions**:

- **Final text only**: store the exact text MIA intends to inject, not raw audio or raw STT, so the
  history matches what the user would copy back into a target app.
- **Local bounded cache**: keep only the latest 100 entries to avoid unbounded app-data growth.
- **Manual controls**: the Hub can copy, remove one item, or clear all history; no cloud sync.

---

## 1. Inputs / Outputs

| Aspect | This feature |
|---|---|
| **Trigger** | A successful non-empty transcription in `stop_dictation`; Hub copy/remove/clear actions |
| **Audio in** | N/A |
| **Text in** | Final cleaned/dictionary/snippet-expanded dictation text |
| **Text out** | Persisted history entry; copied text on explicit user action |
| **Target** | Hub window and OS clipboard |
| **Language** | Language-agnostic UTF-8 |

---

## 2. Engine Contract (Rust)

**Module**: `app/src-tauri/src/history.rs`

```rust
pub fn record_and_save(&self, app: &AppHandle, text: &str) -> Result<(), String>;

#[tauri::command]
fn list_history(state: State<'_, HistoryState>) -> Result<Vec<HistoryEntry>, String>;

#[tauri::command]
fn copy_history_entry(state: State<'_, HistoryState>, id: String) -> Result<(), String>;

#[tauri::command]
fn delete_history_entry(app: AppHandle, state: State<'_, HistoryState>, id: String) -> Result<(), String>;

#[tauri::command]
fn clear_history(app: AppHandle, state: State<'_, HistoryState>) -> Result<(), String>;
```

All commands return `Result<T, String>`. Persistence uses `persist::atomic_write_json`.
Clipboard copy uses `arboard`.

---

## 3. Business Rules

1. **Store final non-empty text** - after cleanup/dictionary/snippets, trim-check; empty text is not stored.
2. **Store before injection** - history survives injection errors or elevated-window rejection.
3. **Newest first** - `list_history` returns most recent entries first.
4. **Bounded list** - adding an entry keeps at most 100 items.
5. **Copy by id** - copying a missing item returns `Err("history entry not found")`.
6. **Delete is idempotent** - deleting a missing id still persists the unchanged list.
7. **Clear removes all entries** - the in-memory state and `history.json` both become empty.

---

## 4. Options & Defaults

| Option | Type | Range / values | Default | Effect |
|---|---|---|---|---|
| Max entries | constant | `100` | `100` | Caps local history size |

---

## 5. Threading / Performance

Recording happens off the audio callback, in `stop_dictation`, after STT has produced text. The
JSON write is small and bounded. Failure to record history does not fail dictation or injection.

---

## 6. UI States

Hub view: loading, empty, populated list, copy success, error banner. Controls are copy, remove,
and clear all.

---

## 7. Edge Cases

| Scenario | Expected behavior |
|---|---|
| Empty dictation | No history entry |
| Injection rejected by elevated window | Final text remains in history |
| Corrupt `history.json` | App starts with empty history |
| Clipboard unavailable | Copy returns a user-visible error |

---

## 8. Testing Checklist

- **Rust**:
  - [x] trimming and empty-skip behavior
  - [x] newest-first bounded insertion
  - [x] delete behavior
- **Manual / runtime**:
  - [ ] speak, then verify the item appears in Hub > Histórico
  - [ ] copy a history item and paste it into another app
  - [ ] remove one item and clear all

---

## 9. Out of Scope

- Search/filter.
- Cloud sync.
- Recording raw audio.
