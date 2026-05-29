//! Silero VAD endpointing (ADR-007, Phase 1) — decides *where speech is* so only
//! speech frames reach the warm STT (anti-hallucination) and so toggle mode knows
//! when an utterance ended. See `docs/specs/audio-capture.md` (§2–§5).
//!
//! This module owns the **endpoint state machine**, which is pure and fully
//! cargo-tested: it consumes a per-frame speech *probability* (Silero's output)
//! and emits `VadDecision` transitions with debounce (`MIN_SPEECH_MS`) and
//! hangover (`MIN_SILENCE_MS`). The Silero model load + per-frame inference that
//! *produces* those probabilities is the runtime seam wired during the capture /
//! orchestrator stage; it is deliberately kept separate from this decision logic
//! so the machine can be tested without a model or a microphone.

/// VAD frame size — Silero classifies small fixed frames (§4). 30 ms at 16 kHz
/// is 480 samples. Kept here because it is a VAD/processing concern, not a device
/// format concern (which lives in `audio.rs`).
pub const FRAME_MS: u32 = 30;
/// Speech-probability threshold above which a frame counts as speech (§4, ~0.5).
pub const VAD_THRESHOLD: f32 = 0.5;
/// Minimum continuous speech before `SpeechStarted` — debounces coughs/clicks (Rule 4).
pub const MIN_SPEECH_MS: u32 = 150;
/// Silence run that ends an utterance in toggle mode (Rule 5).
pub const MIN_SILENCE_MS: u32 = 700;

/// One transition emitted per processed frame (spec §2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VadDecision {
    /// No speech yet (or after an endpoint): nothing to forward.
    Silence,
    /// First frame at which a debounced run of speech became an utterance (Rule 4).
    SpeechStarted,
    /// Inside an utterance — including the hangover window before an endpoint.
    SpeechOngoing,
    /// A silence run reached `MIN_SILENCE_MS`: the utterance ended (Rule 5).
    SpeechEnded,
}

/// Round `ms` up to whole frames of `FRAME_MS` each (≥ 1) — a 700 ms hangover at
/// 30 ms/frame becomes 24 frames, never 0. Pure, so it is unit-tested directly.
fn frames_for_ms(ms: u32, frame_ms: u32) -> u32 {
    let f = frame_ms.max(1);
    ms.div_ceil(f).max(1)
}

/// The pure endpoint state machine (Rules 4, 5, 8). Feed it one speech probability
/// per `FRAME_MS` frame; it returns the transition for that frame. It re-arms after
/// `SpeechEnded`, so toggle mode can detect successive utterances without a reset.
#[derive(Clone, Debug)]
pub struct EndpointDetector {
    threshold: f32,
    min_speech_frames: u32,
    min_silence_frames: u32,
    in_speech: bool,
    speech_run: u32,
    silence_run: u32,
}

impl EndpointDetector {
    /// Build from the fixed defaults (§4).
    pub fn new() -> Self {
        Self::with_params(VAD_THRESHOLD, MIN_SPEECH_MS, MIN_SILENCE_MS, FRAME_MS)
    }

    /// Build from explicit params — used by tests to drive short sequences.
    pub fn with_params(threshold: f32, min_speech_ms: u32, min_silence_ms: u32, frame_ms: u32) -> Self {
        Self {
            threshold,
            min_speech_frames: frames_for_ms(min_speech_ms, frame_ms),
            min_silence_frames: frames_for_ms(min_silence_ms, frame_ms),
            in_speech: false,
            speech_run: 0,
            silence_run: 0,
        }
    }

    /// Discard all run state (e.g. on `stop_capture` / new session).
    pub fn reset(&mut self) {
        self.in_speech = false;
        self.speech_run = 0;
        self.silence_run = 0;
    }

    /// Process one frame's speech probability and return its transition.
    pub fn push(&mut self, speech_prob: f32) -> VadDecision {
        let is_speech = speech_prob >= self.threshold;
        if self.in_speech {
            self.push_in_speech(is_speech)
        } else {
            self.push_pre_speech(is_speech)
        }
    }

    /// Before an utterance: count a debounced run of speech up to `SpeechStarted`.
    fn push_pre_speech(&mut self, is_speech: bool) -> VadDecision {
        if !is_speech {
            self.speech_run = 0;
            return VadDecision::Silence;
        }
        self.speech_run += 1;
        if self.speech_run >= self.min_speech_frames {
            self.in_speech = true;
            self.speech_run = 0;
            self.silence_run = 0;
            return VadDecision::SpeechStarted;
        }
        // Still debouncing — not yet a confirmed utterance (Rule 4).
        VadDecision::Silence
    }

    /// Inside an utterance: hold through the hangover, end after `MIN_SILENCE_MS`.
    fn push_in_speech(&mut self, is_speech: bool) -> VadDecision {
        if is_speech {
            self.silence_run = 0;
            return VadDecision::SpeechOngoing;
        }
        self.silence_run += 1;
        if self.silence_run >= self.min_silence_frames {
            self.in_speech = false;
            self.speech_run = 0;
            self.silence_run = 0;
            return VadDecision::SpeechEnded;
        }
        // Within the hangover window — still part of the utterance (Rule 6/7).
        VadDecision::SpeechOngoing
    }
}

impl Default for EndpointDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(det: &mut EndpointDetector, probs: &[f32]) -> Vec<VadDecision> {
        probs.iter().map(|&p| det.push(p)).collect()
    }

    #[test]
    fn frames_for_ms_rounds_up_and_floors_at_one() {
        assert_eq!(frames_for_ms(150, 30), 5);
        assert_eq!(frames_for_ms(700, 30), 24); // 700/30 = 23.33 -> 24
        assert_eq!(frames_for_ms(1, 30), 1); // never 0
        assert_eq!(frames_for_ms(0, 30), 1); // floored to 1
    }

    #[test]
    fn all_silence_never_starts_speech() {
        // Rule 8: an all-silence session never emits SpeechStarted.
        let mut det = EndpointDetector::with_params(0.5, 150, 700, 30);
        let out = run(&mut det, &[0.0; 50]);
        assert!(out.iter().all(|d| *d == VadDecision::Silence));
    }

    #[test]
    fn speech_starts_only_after_min_speech_frames() {
        // min_speech_frames = 5; SpeechStarted lands on exactly the 5th speech frame (Rule 4).
        let mut det = EndpointDetector::with_params(0.5, 150, 700, 30);
        let out = run(&mut det, &[0.9, 0.9, 0.9, 0.9, 0.9]);
        assert_eq!(
            out,
            vec![
                VadDecision::Silence,
                VadDecision::Silence,
                VadDecision::Silence,
                VadDecision::Silence,
                VadDecision::SpeechStarted,
            ]
        );
    }

    #[test]
    fn short_blip_does_not_start_speech() {
        // 3 speech frames (< 5) then silence → debounced away (Rule 4 / cough edge case).
        let mut det = EndpointDetector::with_params(0.5, 150, 700, 30);
        let out = run(&mut det, &[0.9, 0.9, 0.9, 0.0, 0.0]);
        assert!(!out.contains(&VadDecision::SpeechStarted));
    }

    #[test]
    fn ongoing_then_ends_after_min_silence() {
        let mut det = EndpointDetector::with_params(0.5, 150, 700, 30);
        // start speech (5 frames), one more speech frame, then 24 silence frames.
        run(&mut det, &[0.9; 5]); // -> SpeechStarted on the 5th
        assert_eq!(det.push(0.9), VadDecision::SpeechOngoing);
        // min_silence_frames = 24: frames 1..=23 hold (hangover), frame 24 ends.
        for _ in 0..23 {
            assert_eq!(det.push(0.0), VadDecision::SpeechOngoing);
        }
        assert_eq!(det.push(0.0), VadDecision::SpeechEnded);
    }

    #[test]
    fn re_arms_for_a_second_utterance() {
        // After SpeechEnded the machine must detect a fresh utterance (toggle mode).
        let mut det = EndpointDetector::with_params(0.5, 150, 700, 30);
        run(&mut det, &[0.9; 5]); // utterance 1 start
        for _ in 0..24 {
            det.push(0.0);
        } // utterance 1 end
        let out = run(&mut det, &[0.9; 5]); // utterance 2
        assert_eq!(out.last(), Some(&VadDecision::SpeechStarted));
    }

    #[test]
    fn brief_pause_inside_utterance_does_not_end_it() {
        // A silence run shorter than the hangover keeps the utterance open (Rule 6/7).
        let mut det = EndpointDetector::with_params(0.5, 150, 700, 30);
        run(&mut det, &[0.9; 5]);
        for _ in 0..10 {
            assert_eq!(det.push(0.0), VadDecision::SpeechOngoing); // 10 < 24
        }
        assert_eq!(det.push(0.9), VadDecision::SpeechOngoing); // speech resumes, run resets
        for _ in 0..23 {
            assert_eq!(det.push(0.0), VadDecision::SpeechOngoing);
        }
        assert_eq!(det.push(0.0), VadDecision::SpeechEnded);
    }
}
