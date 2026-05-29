//! Per-app writing styles / context (Phase 3 — per-app-context.md). A small, optional
//! set of overrides keyed to the **focused application's executable name** (resolved at
//! dictation start by `win32::foreground_process_name`): force a language, a clipboard-vs-
//! SendInput backend, a trailing period, or spoken-punctuation on/off for a given app.
//!
//! This module is **pure and cargo-tested**: matching + override resolution have no I/O.
//! The orchestrator (`dictation.rs`) looks up the matching style and applies it; the
//! styles themselves are persisted in `settings.rs` (the `perApp` group).

use serde::{Deserialize, Serialize};

use crate::cleanup::CleanupOptions;
use crate::inject::InjectMode;
use crate::settings::DefaultLanguage;

/// One per-app override rule. `match_exe` is a (case-insensitive) substring of the
/// foreground process' executable stem — `code`, `chrome`, `winword`, `slack`. Every
/// override is optional; `None` means "inherit the global setting" for that field.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AppStyle {
    pub match_exe: String,
    pub language: Option<DefaultLanguage>,
    pub inject_mode: Option<InjectMode>,
    pub ensure_trailing_period: Option<bool>,
    pub spoken_punctuation: Option<bool>,
}

/// The matching rule for an executable stem: case-insensitive substring, **longest
/// `match_exe` wins** (mirrors the dictionary/snippets longest-match idiom) so a specific
/// `visualstudio` beats a generic `studio`. Empty/blank rules never match.
pub fn match_style<'a>(styles: &'a [AppStyle], exe: &str) -> Option<&'a AppStyle> {
    let exe = exe.to_lowercase();
    styles
        .iter()
        .filter(|s| {
            let m = s.match_exe.trim().to_lowercase();
            !m.is_empty() && exe.contains(&m)
        })
        .max_by_key(|s| s.match_exe.trim().len())
}

/// The dictation language for this utterance: the app override, else the global default.
pub fn resolve_language(base: DefaultLanguage, style: Option<&AppStyle>) -> DefaultLanguage {
    style.and_then(|s| s.language).unwrap_or(base)
}

/// The injection backend for this utterance: the app override, else `Auto`.
pub fn resolve_inject_mode(style: Option<&AppStyle>) -> InjectMode {
    style.and_then(|s| s.inject_mode).unwrap_or(InjectMode::Auto)
}

/// Fold an app style's cleanup overrides onto the global cleanup options (only the fields
/// the style sets are changed; the rest are inherited).
pub fn merge_cleanup(mut base: CleanupOptions, style: Option<&AppStyle>) -> CleanupOptions {
    if let Some(s) = style {
        if let Some(v) = s.ensure_trailing_period {
            base.ensure_trailing_period = v;
        }
        if let Some(v) = s.spoken_punctuation {
            base.spoken_punctuation = v;
        }
    }
    base
}

/// Drop blank rules and de-duplicate by (trimmed, lowercased) `match_exe`, keeping the
/// first — defensive normalization for `settings::validate` (the UI is never the only
/// guard). Trims `match_exe` in place so matching is stable.
pub fn sanitize(styles: Vec<AppStyle>) -> Vec<AppStyle> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for mut s in styles {
        s.match_exe = s.match_exe.trim().to_string();
        let key = s.match_exe.to_lowercase();
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        out.push(s);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(exe: &str) -> AppStyle {
        AppStyle { match_exe: exe.to_string(), ..Default::default() }
    }

    #[test]
    fn match_is_case_insensitive_substring() {
        let styles = vec![style("code")];
        assert!(match_style(&styles, "C:\\Code.EXE-resolved-to-CODE").is_some());
        assert!(match_style(&styles, "code").is_some());
        assert!(match_style(&styles, "notepad").is_none());
    }

    #[test]
    fn longest_match_wins() {
        let styles = vec![style("studio"), style("visualstudio")];
        let m = match_style(&styles, "visualstudio").unwrap();
        assert_eq!(m.match_exe, "visualstudio");
    }

    #[test]
    fn blank_rule_never_matches() {
        let styles = vec![style("   ")];
        assert!(match_style(&styles, "anything").is_none());
    }

    #[test]
    fn resolve_language_prefers_override() {
        let s = AppStyle { language: Some(DefaultLanguage::En), ..style("code") };
        assert_eq!(resolve_language(DefaultLanguage::Pt, Some(&s)), DefaultLanguage::En);
        assert_eq!(resolve_language(DefaultLanguage::Pt, None), DefaultLanguage::Pt);
        // No language override → inherit the base even when a style matches.
        assert_eq!(resolve_language(DefaultLanguage::Pt, Some(&style("code"))), DefaultLanguage::Pt);
    }

    #[test]
    fn resolve_inject_mode_defaults_to_auto() {
        let s = AppStyle { inject_mode: Some(InjectMode::Clipboard), ..style("teams") };
        assert_eq!(resolve_inject_mode(Some(&s)), InjectMode::Clipboard);
        assert_eq!(resolve_inject_mode(None), InjectMode::Auto);
        assert_eq!(resolve_inject_mode(Some(&style("teams"))), InjectMode::Auto);
    }

    #[test]
    fn merge_cleanup_applies_only_set_overrides() {
        let base = CleanupOptions {
            spoken_punctuation: true,
            ensure_trailing_period: false,
            ..Default::default()
        };
        let s = AppStyle {
            ensure_trailing_period: Some(true),
            spoken_punctuation: None,
            ..style("winword")
        };
        let merged = merge_cleanup(base.clone(), Some(&s));
        assert!(merged.ensure_trailing_period); // overridden
        assert!(merged.spoken_punctuation); // inherited (None)
        // No style → fields unchanged (CleanupOptions has no PartialEq, so compare fields).
        let merged_none = merge_cleanup(base.clone(), None);
        assert_eq!(merged_none.spoken_punctuation, base.spoken_punctuation);
        assert_eq!(merged_none.ensure_trailing_period, base.ensure_trailing_period);
    }

    #[test]
    fn sanitize_trims_drops_blank_and_dedups() {
        let styles = vec![style("  Code  "), style("code"), style(""), style("chrome")];
        let out = sanitize(styles);
        assert_eq!(out.len(), 2); // "Code" (trimmed) + "chrome"; dup "code" and blank dropped
        assert_eq!(out[0].match_exe, "Code");
        assert_eq!(out[1].match_exe, "chrome");
    }
}
