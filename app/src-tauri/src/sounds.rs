//! Dictation audio feedback — soft bell-envelope sine tones, gated by
//! `settings.general.dictation_sounds`.
//!
//! A dedicated audio thread initialises the output device once at startup and
//! listens on a channel; `on_start`/`on_end` just send a message so the
//! dictation pipeline is never stalled waiting for device init.

use std::sync::{mpsc, OnceLock};

enum Cmd {
    Play { freq: f32, duration_ms: u32 },
}

static TX: OnceLock<mpsc::SyncSender<Cmd>> = OnceLock::new();

/// Spawn the audio thread and open the output device once. Call from `run()`
/// before any dictation command can fire. Silently ignored on a second call.
pub fn init() {
    if TX.get().is_some() {
        return;
    }
    let (tx, rx) = mpsc::sync_channel::<Cmd>(4);
    std::thread::Builder::new()
        .name("mia-sounds".into())
        .spawn(move || {
            // Device init happens once here; if it fails the thread exits
            // silently and every subsequent send is simply dropped.
            let Ok((_stream, handle)) = rodio::OutputStream::try_default() else { return };
            while let Ok(cmd) = rx.recv() {
                let Cmd::Play { freq, duration_ms } = cmd;
                if let Ok(sink) = rodio::Sink::try_new(&handle) {
                    sink.append(rodio::buffer::SamplesBuffer::new(
                        1,
                        44_100,
                        bell(freq, duration_ms),
                    ));
                    // detach: the sink plays to completion in the background;
                    // the audio thread is free to handle the next command.
                    sink.detach();
                }
            }
        })
        .ok();
    TX.set(tx).ok();
}

fn send(cmd: Cmd) {
    if let Some(tx) = TX.get() {
        tx.try_send(cmd).ok();
    }
}

/// Play the dictation-start cue if `sounds_enabled` is true.
pub fn on_start(sounds_enabled: bool) {
    if sounds_enabled {
        send(Cmd::Play { freq: 523.0, duration_ms: 300 });
    }
}

/// Play the dictation-end cue if `sounds_enabled` is true.
pub fn on_end(sounds_enabled: bool) {
    if sounds_enabled {
        send(Cmd::Play { freq: 392.0, duration_ms: 300 });
    }
}

/// Sine wave with bell envelope: fast linear attack then exponential decay.
fn bell(freq: f32, duration_ms: u32) -> Vec<f32> {
    let sample_rate = 44_100u32;
    let n = (sample_rate as f32 * duration_ms as f32 / 1_000.0) as usize;
    let attack = (sample_rate as f32 * 0.008) as usize;
    let decay = 6.0_f32 / (duration_ms as f32 / 1_000.0);

    (0..n)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            let sine = (2.0 * std::f32::consts::PI * freq * t).sin();
            let ramp = if i < attack { i as f32 / attack as f32 } else { 1.0 };
            sine * ramp * (-decay * t).exp() * 0.175
        })
        .collect()
}
