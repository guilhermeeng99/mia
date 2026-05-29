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

use serde::Serialize;

use crate::hotkey::ActivationMode;

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
