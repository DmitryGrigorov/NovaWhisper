//! Energy-based voice activity detection with hysteresis and hangover.
//!
//! Good enough to auto-stop an utterance after trailing silence; a model-based
//! VAD (e.g. Silero) can replace this behind the same interface later.

use crate::audio::TARGET_SAMPLE_RATE;

#[derive(Debug, Clone)]
pub struct VadConfig {
    /// Analysis frame length in ms.
    pub frame_ms: u32,
    /// RMS level (0..1) above which a frame counts as speech.
    pub threshold: f32,
    /// Continuous speech required before reporting SpeechStart.
    pub min_speech_ms: u32,
    /// Trailing silence required before reporting SpeechEnd.
    pub hangover_ms: u32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            frame_ms: 20,
            threshold: 0.015,
            min_speech_ms: 120,
            hangover_ms: 800,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadEvent {
    SpeechStart,
    SpeechEnd,
}

pub struct Vad {
    cfg: VadConfig,
    frame_len: usize,
    buf: Vec<i16>,
    in_speech: bool,
    speech_run_ms: u32,
    silence_run_ms: u32,
}

impl Vad {
    pub fn new(cfg: VadConfig) -> Self {
        let frame_len = (TARGET_SAMPLE_RATE * cfg.frame_ms / 1000) as usize;
        Self {
            cfg,
            frame_len,
            buf: Vec::new(),
            in_speech: false,
            speech_run_ms: 0,
            silence_run_ms: 0,
        }
    }

    pub fn is_speaking(&self) -> bool {
        self.in_speech
    }

    /// Feed 16 kHz mono samples; returns at most one event per call batch.
    pub fn push(&mut self, samples: &[i16]) -> Option<VadEvent> {
        self.buf.extend_from_slice(samples);
        let mut event = None;
        while self.buf.len() >= self.frame_len {
            let frame: Vec<i16> = self.buf.drain(..self.frame_len).collect();
            if let Some(e) = self.push_frame(&frame) {
                event = Some(e);
            }
        }
        event
    }

    fn push_frame(&mut self, frame: &[i16]) -> Option<VadEvent> {
        let rms = (frame
            .iter()
            .map(|s| {
                let f = *s as f32 / i16::MAX as f32;
                f * f
            })
            .sum::<f32>()
            / frame.len().max(1) as f32)
            .sqrt();

        let voiced = rms >= self.cfg.threshold;
        if voiced {
            self.speech_run_ms += self.cfg.frame_ms;
            self.silence_run_ms = 0;
            if !self.in_speech && self.speech_run_ms >= self.cfg.min_speech_ms {
                self.in_speech = true;
                return Some(VadEvent::SpeechStart);
            }
        } else {
            self.silence_run_ms += self.cfg.frame_ms;
            self.speech_run_ms = 0;
            if self.in_speech && self.silence_run_ms >= self.cfg.hangover_ms {
                self.in_speech = false;
                return Some(VadEvent::SpeechEnd);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(ms: u32, amplitude: f32) -> Vec<i16> {
        let n = (TARGET_SAMPLE_RATE * ms / 1000) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / TARGET_SAMPLE_RATE as f32;
                ((t * 440.0 * std::f32::consts::TAU).sin() * amplitude * i16::MAX as f32)
                    as i16
            })
            .collect()
    }

    fn silence(ms: u32) -> Vec<i16> {
        vec![0i16; (TARGET_SAMPLE_RATE * ms / 1000) as usize]
    }

    #[test]
    fn detects_speech_start_and_end() {
        let mut vad = Vad::new(VadConfig::default());
        assert_eq!(vad.push(&silence(200)), None);
        assert_eq!(vad.push(&tone(300, 0.3)), Some(VadEvent::SpeechStart));
        assert!(vad.is_speaking());
        assert_eq!(vad.push(&silence(1000)), Some(VadEvent::SpeechEnd));
        assert!(!vad.is_speaking());
    }

    #[test]
    fn short_blip_does_not_trigger() {
        let mut vad = Vad::new(VadConfig::default());
        assert_eq!(vad.push(&tone(40, 0.3)), None);
        assert_eq!(vad.push(&silence(500)), None);
    }

    #[test]
    fn short_pause_does_not_end_speech() {
        let mut vad = Vad::new(VadConfig::default());
        assert_eq!(vad.push(&tone(300, 0.3)), Some(VadEvent::SpeechStart));
        assert_eq!(vad.push(&silence(300)), None);
        assert!(vad.is_speaking());
        assert_eq!(vad.push(&tone(300, 0.3)), None);
    }
}
