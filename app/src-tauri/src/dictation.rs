//! Dictation orchestrator (the pipeline conductor, Phase 1). Coordinates
//! hotkey → capture → VAD → STT → cleanup → dictionary → snippets → inject and
//! drives the HUD state machine. See `docs/specs/dictation.md`.
//!
//! This file is the **pure, cargo-tested core**: the `next_phase` HUD state
//! machine, the trigger-mode `interpret_down` interpreter, the `classify_cancel`
//! reason classifier, and the `build_result` latency-summary builder — all with no
//! I/O (vad/hotkey pattern). The `start/stop/cancel_dictation` commands that wire
//! the real cpal capture + warm STT + injection are runtime-pending: they depend on
//! the audio-capture runtime, and are best validated on Windows with a mic.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};

use crate::hotkey::ActivationMode;
use crate::settings::{CleanupSettings, DefaultLanguage};

/// The HUD/orchestrator phase (spec §2). `Error` is transient → `Idle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Idle,
    Listening,
    Transcribing,
    Inserting,
    Error,
}

/// What happened to drive a phase transition (the state machine's input alphabet).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    /// Hotkey-down / trigger → begin a session.
    Start,
    /// Capture ended (release, toggle-off, or VAD endpoint) → transcribe.
    EndCapture,
    /// VAD found no speech this session → nothing to transcribe (Rule 7).
    Empty,
    /// STT produced non-empty cleaned text → inject it.
    Transcribed,
    /// STT/cleanup reduced to empty → inject nothing (Rule 7).
    TranscribedEmpty,
    /// Injection finished → back to Idle.
    Injected,
    /// Escape / abort at any phase (Rule 8). Idempotent.
    Cancel,
    /// A stage failed (mic / STT / injection) → transient Error (Rule 14).
    Fail,
    /// Dismiss the transient Error → Idle.
    Dismiss,
}

/// Why a session ended without injecting (spec §2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CancelReason {
    UserEscape,
    EmptySpeech,
    Timeout,
}

/// What a hotkey-down means given the current session (Rule 12). `Start` begins a
/// session (idle); during an active session, toggle-mode stops it and hold-mode
/// ignores the auto-repeat. (A fresh `start_dictation` while active is rejected at
/// the command layer — "dictation already active".)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionAction {
    Start,
    Stop,
    Ignore,
}

/// Streamed to the HUD over a Tauri `Channel` (spec §2). `tag = "kind"`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum DictationEvent {
    StateChanged { phase: Phase },
    Level { rms: f32 },
    Injected { chars: usize, ms: u64 },
    Cancelled { reason: CancelReason },
    Error { message: String },
}

/// The end-to-end session summary (spec §2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationResult {
    pub injected_chars: usize,
    pub detected_language: Option<String>,
    pub total_ms: u64,
    pub stt_ms: u64,
    pub backend: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Pure core (cargo-tested)
// ─────────────────────────────────────────────────────────────────────────────

/// The HUD state machine (spec §6, Rules 1-2, 7-8, 14). A signal that doesn't apply
/// to the current phase leaves it unchanged (defensive — no illegal transitions).
pub fn next_phase(phase: Phase, signal: Signal) -> Phase {
    use Phase::*;
    use Signal::*;
    // Escape/abort always returns to Idle from any phase (Rule 8, idempotent).
    if signal == Cancel {
        return Idle;
    }
    match (phase, signal) {
        (Idle, Start) => Listening,
        (Listening, EndCapture) => Transcribing,
        (Listening, Empty) => Idle,
        (Transcribing, Transcribed) => Inserting,
        (Transcribing, TranscribedEmpty) => Idle,
        (Inserting, Injected) => Idle,
        (_, Fail) => Error,
        (Error, Dismiss) => Idle,
        // Any other (phase, signal) pair is a no-op.
        (current, _) => current,
    }
}

/// Interpret a hotkey-down given whether a session is already active (Rule 12).
pub fn interpret_down(mode: ActivationMode, active: bool) -> SessionAction {
    match (active, mode) {
        (false, _) => SessionAction::Start,
        (true, ActivationMode::PressToToggle) => SessionAction::Stop,
        (true, ActivationMode::PushToHold) => SessionAction::Ignore,
    }
}

/// Classify why a session ended without injecting (spec §2). Escape wins over a
/// timeout, which wins over plain empty speech.
pub fn classify_cancel(escaped: bool, timed_out: bool) -> CancelReason {
    if escaped {
        CancelReason::UserEscape
    } else if timed_out {
        CancelReason::Timeout
    } else {
        CancelReason::EmptySpeech
    }
}

/// Assemble the latency summary from stage timestamps (ms, monotonic). `total_ms`
/// is capture-start → done; `stt_ms` is the inference portion. Saturating so a
/// non-monotonic clock can never underflow.
pub fn build_result(
    injected_chars: usize,
    detected_language: Option<String>,
    capture_start_ms: u64,
    stt_start_ms: u64,
    stt_end_ms: u64,
    done_ms: u64,
    backend: &str,
) -> DictationResult {
    DictationResult {
        injected_chars,
        detected_language,
        total_ms: done_ms.saturating_sub(capture_start_ms),
        stt_ms: stt_end_ms.saturating_sub(stt_start_ms),
        backend: backend.to_string(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Orchestrator commands (wire the real pipeline; runtime-validated on Windows)
// ─────────────────────────────────────────────────────────────────────────────
//
// MVP shape: start_dictation opens capture and returns; stop_dictation runs the
// whole tail (STT → cleanup → dictionary → snippets → inject) and returns the
// summary. Live HUD waveform Level wiring (forwarding CaptureEvent::Level into the
// DictationEvent channel) is a follow-up; the core loop works without it. The
// anti-hallucination VAD is applied by whisper-server at transcription time.

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn today_days() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| (d.as_secs() / 86_400) as i64).unwrap_or(0)
}

fn stt_lang(lang: DefaultLanguage) -> Option<String> {
    match lang {
        DefaultLanguage::Auto => None,
        DefaultLanguage::Pt => Some("pt".to_string()),
        DefaultLanguage::En => Some("en".to_string()),
    }
}

fn cleanup_lang(lang: DefaultLanguage) -> crate::cleanup::Lang {
    match lang {
        DefaultLanguage::Auto => crate::cleanup::Lang::Other,
        DefaultLanguage::Pt => crate::cleanup::Lang::PtBr,
        DefaultLanguage::En => crate::cleanup::Lang::En,
    }
}

fn cleanup_options(c: &CleanupSettings) -> crate::cleanup::CleanupOptions {
    crate::cleanup::CleanupOptions {
        remove_fillers: c.filler_removal,
        spoken_punctuation: c.spoken_punctuation,
        collapse_repeats: c.stutter_collapse,
        fix_capitalization: c.capitalization,
        normalize_numbers: true,
        ensure_trailing_period: false,
        extra_fillers: Vec::new(),
        keep_fillers: Vec::new(),
    }
}

fn empty_result() -> DictationResult {
    DictationResult {
        injected_chars: 0,
        detected_language: None,
        total_ms: 0,
        stt_ms: 0,
        backend: "none".to_string(),
    }
}

/// Emit a terminal HUD event (Cancelled/Error) then always settle the HUD to Idle —
/// the single tail every non-injecting exit path shares (Rules 7-8, 14).
fn emit_then_idle(events: &Channel<DictationEvent>, ev: DictationEvent) {
    let _ = events.send(ev);
    let _ = events.send(DictationEvent::StateChanged { phase: Phase::Idle });
}

/// Phase mirrored to the floating HUD window over the global `hud://state` event.
/// The HUD lives in its own webview (hud.rs) and is driven by the engine directly —
/// not relayed through the main window — so it works even when the Hub is hidden.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct HudState<'a> {
    phase: Phase,
    message: Option<&'a str>,
}

fn emit_hud(app: &AppHandle, phase: Phase, message: Option<&str>) {
    let _ = app.emit("hud://state", HudState { phase, message });
}

/// Begin a session: open mic capture and show the HUD listening state (Rule 1).
/// Returns immediately; `stop_dictation` runs the tail. (push-to-hold MVP)
#[tauri::command]
pub fn start_dictation(
    app: AppHandle,
    capture: State<'_, crate::audio::CaptureState>,
    settings: State<'_, crate::settings::SettingsState>,
    events: Channel<DictationEvent>,
) -> Result<(), String> {
    let s = settings.snapshot()?;
    eprintln!("[dictation] start: opening capture (device={:?})", s.audio.input_device);
    // Live waveform Level forwarding is a follow-up; capture runs without a channel.
    crate::audio::begin_capture(&capture, Some(&s.audio.input_device), None)?;
    let _ = events.send(DictationEvent::StateChanged { phase: Phase::Listening });
    emit_hud(&app, Phase::Listening, None);
    Ok(())
}

/// End capture and run the pipeline: STT → cleanup → dictionary → snippets →
/// inject, emitting HUD events and returning the summary (Rules 2, 5-10, 14).
#[tauri::command]
#[allow(clippy::too_many_arguments)] // each managed State is a distinct collaborator
pub fn stop_dictation(
    app: AppHandle,
    capture: State<'_, crate::audio::CaptureState>,
    stt_state: State<'_, crate::stt::SttState>,
    settings: State<'_, crate::settings::SettingsState>,
    dict: State<'_, crate::dictionary::DictState>,
    snips: State<'_, crate::snippets::SnippetState>,
    stats: State<'_, crate::stats::StatsState>,
    events: Channel<DictationEvent>,
) -> Result<DictationResult, String> {
    let s = settings.snapshot()?;
    let samples = crate::audio::end_capture(&capture)?;
    eprintln!("[dictation] stop: captured {} samples (~{:.1}s)", samples.len(), samples.len() as f32 / 16_000.0);
    if samples.is_empty() {
        emit_then_idle(&events, DictationEvent::Cancelled { reason: CancelReason::EmptySpeech });
        emit_hud(&app, Phase::Idle, None);
        return Ok(empty_result());
    }

    let _ = events.send(DictationEvent::StateChanged { phase: Phase::Transcribing });
    emit_hud(&app, Phase::Transcribing, None);
    let t0 = now_ms();
    let app_for_err = app.clone();
    let fail = |events: &Channel<DictationEvent>, e: String| -> String {
        eprintln!("[dictation] error: {e}");
        emit_hud(&app_for_err, Phase::Error, Some(&e));
        emit_then_idle(events, DictationEvent::Error { message: e.clone() });
        e
    };

    crate::stt::warm_model(&app, &stt_state, &s.model.model).map_err(|e| fail(&events, e))?;
    eprintln!("[dictation] warm engine ready (model={})", s.model.model);
    let stt_start = now_ms();
    let lang = stt_lang(s.general.default_language);
    // Snapshot the dictionary once: its bias terms become Whisper's initial prompt
    // (spelling nudge) AND the post-STT replacement set (custom-dictionary.md).
    let (entries, dsettings) = dict.snapshot()?;
    let bias = crate::dictionary::build_bias_prompt(&entries, &dsettings);
    let raw = crate::stt::transcribe_chunk(&stt_state, &samples, lang.as_deref(), (!bias.is_empty()).then_some(bias.as_str()))
        .map_err(|e| fail(&events, e))?;
    let stt_end = now_ms();
    eprintln!("[dictation] transcript: {} chars in {} ms", raw.chars().count(), stt_end.saturating_sub(stt_start));

    let cleaned = crate::cleanup::clean(&raw, cleanup_lang(s.general.default_language), &cleanup_options(&s.cleanup));
    let dicted = crate::dictionary::apply_dictionary(&cleaned, &entries, &dsettings);
    let set = crate::snippets::compile_snippets(&snips.snapshot()?);
    let final_text = crate::snippets::expand_snippets(&dicted, &set).output;

    if final_text.trim().is_empty() {
        eprintln!("[dictation] nothing to inject (empty after cleanup)");
        emit_then_idle(&events, DictationEvent::Cancelled { reason: CancelReason::EmptySpeech });
        emit_hud(&app, Phase::Idle, None);
        return Ok(empty_result());
    }

    let _ = events.send(DictationEvent::StateChanged { phase: Phase::Inserting });
    emit_hud(&app, Phase::Inserting, None);
    crate::inject::inject(&final_text, crate::inject::InjectMode::Auto, &crate::inject::InjectSettings::default())
        .map_err(|e| fail(&events, e))?;
    let done = now_ms();
    eprintln!("[dictation] injected {} chars", final_text.chars().count());

    let chars = final_text.chars().count();
    let elapsed = done.saturating_sub(t0);
    let _ = stats.record_and_save(&app, crate::stats::count_words(&final_text), elapsed, today_days());
    let _ = events.send(DictationEvent::Injected { chars, ms: elapsed });
    let _ = events.send(DictationEvent::StateChanged { phase: Phase::Idle });
    emit_hud(&app, Phase::Idle, None);
    Ok(build_result(chars, lang, t0, stt_start, stt_end, done, "auto"))
}

/// Abort: discard the in-flight session, inject nothing, HUD → Idle (Rule 8).
#[tauri::command]
pub fn cancel_dictation(
    app: AppHandle,
    capture: State<'_, crate::audio::CaptureState>,
    events: Channel<DictationEvent>,
) -> Result<(), String> {
    let _ = crate::audio::end_capture(&capture); // discard the buffer
    emit_then_idle(&events, DictationEvent::Cancelled { reason: CancelReason::UserEscape });
    emit_hud(&app, Phase::Idle, None);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_transitions() {
        let p = Phase::Idle;
        let p = next_phase(p, Signal::Start);
        assert_eq!(p, Phase::Listening);
        let p = next_phase(p, Signal::EndCapture);
        assert_eq!(p, Phase::Transcribing);
        let p = next_phase(p, Signal::Transcribed);
        assert_eq!(p, Phase::Inserting);
        let p = next_phase(p, Signal::Injected);
        assert_eq!(p, Phase::Idle);
    }

    #[test]
    fn empty_speech_returns_to_idle_without_inserting() {
        assert_eq!(next_phase(Phase::Listening, Signal::Empty), Phase::Idle);
        assert_eq!(next_phase(Phase::Transcribing, Signal::TranscribedEmpty), Phase::Idle);
    }

    #[test]
    fn cancel_from_any_phase_returns_idle() {
        for p in [Phase::Idle, Phase::Listening, Phase::Transcribing, Phase::Inserting, Phase::Error] {
            assert_eq!(next_phase(p, Signal::Cancel), Phase::Idle);
        }
    }

    #[test]
    fn fail_goes_to_error_then_dismiss_to_idle() {
        assert_eq!(next_phase(Phase::Transcribing, Signal::Fail), Phase::Error);
        assert_eq!(next_phase(Phase::Error, Signal::Dismiss), Phase::Idle);
    }

    #[test]
    fn illegal_signal_is_a_no_op() {
        // Injected while Idle, EndCapture while Transcribing — neither applies.
        assert_eq!(next_phase(Phase::Idle, Signal::Injected), Phase::Idle);
        assert_eq!(next_phase(Phase::Transcribing, Signal::EndCapture), Phase::Transcribing);
    }

    #[test]
    fn interpret_down_by_mode_and_activity() {
        assert_eq!(interpret_down(ActivationMode::PushToHold, false), SessionAction::Start);
        assert_eq!(interpret_down(ActivationMode::PressToToggle, false), SessionAction::Start);
        assert_eq!(interpret_down(ActivationMode::PressToToggle, true), SessionAction::Stop);
        assert_eq!(interpret_down(ActivationMode::PushToHold, true), SessionAction::Ignore);
    }

    #[test]
    fn classify_cancel_priority() {
        assert_eq!(classify_cancel(true, true), CancelReason::UserEscape);
        assert_eq!(classify_cancel(false, true), CancelReason::Timeout);
        assert_eq!(classify_cancel(false, false), CancelReason::EmptySpeech);
    }

    #[test]
    fn build_result_computes_durations() {
        let r = build_result(42, Some("pt".into()), 1_000, 1_200, 1_900, 2_050, "enigo");
        assert_eq!(r.total_ms, 1_050); // 2050 - 1000
        assert_eq!(r.stt_ms, 700); // 1900 - 1200
        assert_eq!(r.injected_chars, 42);
        assert_eq!(r.backend, "enigo");
    }

    #[test]
    fn build_result_saturates_on_backwards_clock() {
        let r = build_result(0, None, 2_000, 0, 0, 1_000, "clipboard");
        assert_eq!(r.total_ms, 0); // done < start → saturating 0, never underflow
    }
}
