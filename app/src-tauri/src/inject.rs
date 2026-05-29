//! System-wide text injection on Windows (ADR-005) — the last dictation stage.
//! Two backends behind one `TextInjector` trait, runtime-selected: `enigo`
//! `SendInput` Unicode keystrokes (default; layout-independent, no clipboard) and
//! `arboard` clipboard + simulated `Ctrl+V` (fallback / forced; **saves and
//! restores** the user's prior clipboard). See `docs/specs/text-injection.md`.
//!
//! Focused-target and elevated-window (UIPI) detection (spec Rules 6-7) are wired in the
//! **dictation orchestrator** (`dictation.rs`), not here, via `win32.rs`: it forces the
//! clipboard backend when no foreground window is detectable (Rule 6, best-effort) and
//! returns the run-as-administrator error when the target outranks MIA (Rule 7). These
//! backends stay focus-agnostic — they just type into whatever currently has focus.

use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use serde::{Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;

/// Backend selection requested by the caller (`Auto` resolves via `pick_backend`).
/// `Serialize` so it can be persisted as a per-app style override (per-app-context.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InjectMode {
    Auto,
    SendInput,
    Clipboard,
}

/// Resolved backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    SendInput,
    Clipboard,
}

/// Injection options (mirrors the Hub settings; see `docs/specs/settings.md`).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InjectSettings {
    pub force_clipboard_mode: bool,
    pub clipboard_threshold_chars: usize,
    pub sendinput_chunk_chars: usize,
    pub restore_clipboard: bool,
}

impl Default for InjectSettings {
    fn default() -> Self {
        Self {
            force_clipboard_mode: false,
            clipboard_threshold_chars: 1000,
            sendinput_chunk_chars: 64,
            restore_clipboard: true,
        }
    }
}

/// One trait localizing all OS-specific injection (ADR-005 / ADR-011).
pub trait TextInjector: Send + Sync {
    fn inject(&self, text: &str) -> Result<(), String>;
    fn name(&self) -> &'static str;
}

// ─────────────────────────────────────────────────────────────────────────────
// Pure helpers (cargo-tested)
// ─────────────────────────────────────────────────────────────────────────────

/// At/over the threshold, the clipboard backend is preferred (Rule 2b). The
/// threshold is clamped to its documented range defensively.
fn should_use_clipboard(len: usize, threshold: usize) -> bool {
    len >= threshold.clamp(200, 5000)
}

/// The `Auto` decision as a pure function (Rules 1, 2, 11).
fn pick_backend(mode: InjectMode, len: usize, settings: &InjectSettings) -> Backend {
    match mode {
        InjectMode::SendInput => Backend::SendInput,
        InjectMode::Clipboard => Backend::Clipboard,
        InjectMode::Auto => {
            if settings.force_clipboard_mode
                || should_use_clipboard(len, settings.clipboard_threshold_chars)
            {
                Backend::Clipboard
            } else {
                Backend::SendInput
            }
        }
    }
}

/// Split text into `max`-char chunks **on grapheme boundaries** — never mid-scalar,
/// mid-surrogate, or mid-combining-sequence (Rules 9-10). `max` is clamped to its
/// documented range. Empty input → empty vec.
fn chunk_for_sendinput(text: &str, max: usize) -> Vec<&str> {
    let max = max.clamp(16, 512);
    if text.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut start = 0usize;
    let mut count = 0usize;
    for (i, g) in text.grapheme_indices(true) {
        if count >= max {
            chunks.push(&text[start..i]);
            start = i;
            count = 0;
        }
        count += g.chars().count();
    }
    if start < text.len() {
        chunks.push(&text[start..]);
    }
    chunks
}

/// A length-only, non-verbatim placeholder for logs — the transcript is sensitive
/// and is NEVER logged verbatim (Rule 12). The redaction contract lives here, next
/// to the backends it guards, and the dictation orchestrator routes its transcript
/// trace through this helper (`dictation.rs`) so the two can't drift apart.
pub fn redact_for_log(text: &str) -> String {
    format!("<{} chars>", text.chars().count())
}

// ─────────────────────────────────────────────────────────────────────────────
// Backends
// ─────────────────────────────────────────────────────────────────────────────

/// Default backend: `SendInput` with `KEYEVENTF_UNICODE` (via enigo), chunked with
/// a small inter-chunk yield so long paragraphs don't overflow the input queue.
pub struct SendInputInjector {
    pub chunk: usize,
}

impl TextInjector for SendInputInjector {
    fn inject(&self, text: &str) -> Result<(), String> {
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| format!("injection backend failed: {e}"))?;
        for piece in chunk_for_sendinput(text, self.chunk) {
            enigo.text(piece).map_err(|e| format!("injection backend failed: {e}"))?;
            std::thread::sleep(std::time::Duration::from_millis(4));
        }
        Ok(())
    }
    fn name(&self) -> &'static str {
        "send_input"
    }
}

/// Fallback/forced backend: set the clipboard, synthesize `Ctrl+V`, then **restore**
/// the user's prior clipboard (Rules 3-5). Restore is attempted even if paste fails.
pub struct ClipboardInjector;

impl TextInjector for ClipboardInjector {
    fn inject(&self, text: &str) -> Result<(), String> {
        let mut cb = arboard::Clipboard::new().map_err(|_| "clipboard unavailable".to_string())?;
        let saved = cb.get_text().ok();
        cb.set_text(text.to_string()).map_err(|_| "clipboard unavailable".to_string())?;

        let pasted = paste_shortcut();
        // Restore the prior clipboard regardless of paste outcome (Rule 4).
        let restored = match saved {
            Some(prev) => cb.set_text(prev).map_err(|_| "clipboard restore failed".to_string()),
            None => Ok(()),
        };
        pasted?;
        restored?;
        Ok(())
    }
    fn name(&self) -> &'static str {
        "clipboard"
    }
}

fn paste_shortcut() -> Result<(), String> {
    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| format!("injection backend failed: {e}"))?;
    enigo.key(Key::Control, Direction::Press).map_err(|e| format!("injection backend failed: {e}"))?;
    enigo.key(Key::Unicode('v'), Direction::Click).map_err(|e| format!("injection backend failed: {e}"))?;
    enigo.key(Key::Control, Direction::Release).map_err(|e| format!("injection backend failed: {e}"))?;
    Ok(())
}

/// Inject cleaned text into the focused window. Empty/whitespace → no-op (Rule 8).
/// Called in-process by the dictation orchestrator (the hot path) and by the Hub's
/// test command below.
pub fn inject(text: &str, mode: InjectMode, settings: &InjectSettings) -> Result<(), String> {
    if text.trim().is_empty() {
        return Ok(());
    }
    match pick_backend(mode, text.chars().count(), settings) {
        Backend::SendInput => SendInputInjector { chunk: settings.sendinput_chunk_chars }.inject(text),
        Backend::Clipboard => ClipboardInjector.inject(text),
    }
}

/// Hub "test injection" command (and manual mode-forcing). Live dictation calls
/// `inject` directly in Rust and never round-trips through the webview.
#[tauri::command]
pub fn inject_text(text: String, mode: Option<InjectMode>) -> Result<(), String> {
    inject(&text, mode.unwrap_or(InjectMode::Auto), &InjectSettings::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_use_clipboard_boundary() {
        // default threshold 1000
        assert!(!should_use_clipboard(999, 1000));
        assert!(should_use_clipboard(1000, 1000));
        assert!(should_use_clipboard(1001, 1000));
        // threshold clamps below 200
        assert!(should_use_clipboard(200, 10)); // 10 -> clamped to 200
    }

    #[test]
    fn pick_backend_auto_threshold() {
        let s = InjectSettings::default();
        assert_eq!(pick_backend(InjectMode::Auto, 10, &s), Backend::SendInput);
        assert_eq!(pick_backend(InjectMode::Auto, 1000, &s), Backend::Clipboard);
    }

    #[test]
    fn pick_backend_force_clipboard() {
        let s = InjectSettings { force_clipboard_mode: true, ..Default::default() };
        assert_eq!(pick_backend(InjectMode::Auto, 1, &s), Backend::Clipboard);
    }

    #[test]
    fn pick_backend_explicit_overrides() {
        let s = InjectSettings { force_clipboard_mode: true, ..Default::default() };
        // explicit SendInput wins even with force_clipboard on
        assert_eq!(pick_backend(InjectMode::SendInput, 9999, &s), Backend::SendInput);
        assert_eq!(pick_backend(InjectMode::Clipboard, 1, &InjectSettings::default()), Backend::Clipboard);
    }

    #[test]
    fn chunk_splits_on_size() {
        assert_eq!(chunk_for_sendinput("abcdefgh", 16), vec!["abcdefgh"]); // max clamps up to 16
        // force small via clamp floor (16): a 40-char string → 3 chunks of <=16
        let s = "a".repeat(40);
        let chunks = chunk_for_sendinput(&s, 16);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks.iter().map(|c| c.len()).sum::<usize>(), 40);
    }

    #[test]
    fn chunk_empty_is_empty() {
        assert!(chunk_for_sendinput("", 64).is_empty());
    }

    #[test]
    fn chunk_never_splits_grapheme() {
        // base 'e' + combining acute = one grapheme; must never be split apart.
        let g = "e\u{301}"; // é
        let s = g.repeat(20); // 20 graphemes, 40 scalars
        for chunk in chunk_for_sendinput(&s, 16) {
            // every chunk must contain whole graphemes (even count of bytes pattern)
            assert!(chunk.graphemes(true).all(|gr| gr == g));
        }
    }

    #[test]
    fn chunk_keeps_emoji_intact() {
        let s = "a😀b😀c";
        let joined: String = chunk_for_sendinput(s, 16).concat();
        assert_eq!(joined, s);
    }

    #[test]
    fn redact_is_length_only() {
        let r = redact_for_log("super secret password");
        assert_eq!(r, "<21 chars>");
        assert!(!r.contains("secret"));
    }
}
