//! Microphone capture: cpal input stream -> 16 kHz mono i16 chunks.
//!
//! The cpal `Stream` is `!Send`, so it lives on a dedicated thread; audio
//! chunks are handed to async land through a tokio mpsc channel.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tokio::sync::mpsc;

/// Everything downstream (VAD, protocol, STT) works at this rate.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Streaming linear resampler (arbitrary rate -> arbitrary rate, mono f32).
pub struct LinearResampler {
    step: f64,
    buf: Vec<f32>,
    pos: f64,
}

impl LinearResampler {
    pub fn new(src_rate: u32, dst_rate: u32) -> Self {
        Self {
            step: src_rate as f64 / dst_rate as f64,
            buf: Vec::new(),
            pos: 0.0,
        }
    }

    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        self.buf.extend_from_slice(input);
        while (self.pos.floor() as usize) + 1 < self.buf.len() {
            let i = self.pos.floor() as usize;
            let frac = (self.pos - i as f64) as f32;
            out.push(self.buf[i] * (1.0 - frac) + self.buf[i + 1] * frac);
            self.pos += self.step;
        }
        let consumed = (self.pos.floor() as usize).min(self.buf.len());
        self.buf.drain(..consumed);
        self.pos -= consumed as f64;
    }
}

/// Convert arbitrary-rate mono f32 samples to 16 kHz i16 in one shot.
pub fn resample_to_target(samples: &[f32], src_rate: u32) -> Vec<i16> {
    let mut out = Vec::new();
    if src_rate == TARGET_SAMPLE_RATE {
        out = samples.to_vec();
    } else {
        let mut rs = LinearResampler::new(src_rate, TARGET_SAMPLE_RATE);
        rs.process(samples, &mut out);
    }
    out.iter().map(|s| f32_to_i16(*s)).collect()
}

fn f32_to_i16(s: f32) -> i16 {
    (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

/// Handle to a live capture. Dropping it (or calling `stop`) ends capture.
pub struct CaptureHandle {
    pub audio_rx: mpsc::Receiver<Vec<i16>>,
    stop_tx: Option<std::sync::mpsc::Sender<()>>,
    level_bits: Arc<AtomicU32>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl CaptureHandle {
    /// Most recent input RMS level in 0.0..=1.0 (for a mic meter).
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level_bits.load(Ordering::Relaxed))
    }

    /// Shareable level probe that outlives borrows of the handle.
    pub fn level_probe(&self) -> LevelProbe {
        LevelProbe(self.level_bits.clone())
    }

    pub fn stop(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Clone)]
pub struct LevelProbe(Arc<AtomicU32>);

impl LevelProbe {
    pub fn level(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
}

pub struct AudioCapture;

impl AudioCapture {
    /// Start capturing from the default input device, delivering 16 kHz mono
    /// i16 chunks of `chunk_ms` milliseconds each.
    pub fn start(chunk_ms: u32) -> Result<CaptureHandle> {
        let (audio_tx, audio_rx) = mpsc::channel::<Vec<i16>>(128);
        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<()>>();
        let level_bits = Arc::new(AtomicU32::new(0));
        let level_for_thread = level_bits.clone();

        let join = std::thread::Builder::new()
            .name("whispr-capture".into())
            .spawn(move || {
                capture_thread(chunk_ms, audio_tx, stop_rx, ready_tx, level_for_thread);
            })
            .context("failed to spawn capture thread")?;

        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(CaptureHandle {
                audio_rx,
                stop_tx: Some(stop_tx),
                level_bits,
                join: Some(join),
            }),
            Ok(Err(e)) => {
                let _ = join.join();
                Err(e)
            }
            Err(_) => Err(anyhow!("audio capture did not start within 5s")),
        }
    }
}

fn capture_thread(
    chunk_ms: u32,
    audio_tx: mpsc::Sender<Vec<i16>>,
    stop_rx: std::sync::mpsc::Receiver<()>,
    ready_tx: std::sync::mpsc::Sender<Result<()>>,
    level_bits: Arc<AtomicU32>,
) {
    let build = || -> Result<(cpal::Stream, u32)> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow!("no input audio device found"))?;
        let config = device
            .default_input_config()
            .context("no default input config")?;
        let src_rate = config.sample_rate().0;
        let channels = config.channels() as usize;
        let chunk_samples = (TARGET_SAMPLE_RATE * chunk_ms / 1000) as usize;

        let mut resampler = LinearResampler::new(src_rate, TARGET_SAMPLE_RATE);
        let mut mono = Vec::<f32>::new();
        let mut resampled = Vec::<f32>::new();
        let mut pending = Vec::<i16>::with_capacity(chunk_samples * 2);
        let tx = audio_tx.clone();

        let mut on_block = move |mono_block: &[f32]| {
            let rms = (mono_block.iter().map(|s| s * s).sum::<f32>()
                / mono_block.len().max(1) as f32)
                .sqrt();
            level_bits.store(rms.to_bits(), Ordering::Relaxed);

            resampled.clear();
            resampler.process(mono_block, &mut resampled);
            pending.extend(resampled.iter().map(|s| f32_to_i16(*s)));
            while pending.len() >= chunk_samples {
                let chunk: Vec<i16> = pending.drain(..chunk_samples).collect();
                // Never block the audio callback; drop the chunk if the
                // consumer is behind.
                let _ = tx.try_send(chunk);
            }
        };

        let err_fn = |e| tracing::error!("audio stream error: {e}");
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                move |data: &[f32], _| {
                    mix_mono(data, channels, &mut mono);
                    on_block(&mono);
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::I16 => device.build_input_stream(
                &config.into(),
                move |data: &[i16], _| {
                    let f: Vec<f32> =
                        data.iter().map(|s| *s as f32 / i16::MAX as f32).collect();
                    mix_mono(&f, channels, &mut mono);
                    on_block(&mono);
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::U16 => device.build_input_stream(
                &config.into(),
                move |data: &[u16], _| {
                    let f: Vec<f32> = data
                        .iter()
                        .map(|s| (*s as f32 - 32768.0) / 32768.0)
                        .collect();
                    mix_mono(&f, channels, &mut mono);
                    on_block(&mono);
                },
                err_fn,
                None,
            )?,
            other => return Err(anyhow!("unsupported sample format {other:?}")),
        };
        stream.play().context("failed to start input stream")?;
        Ok((stream, src_rate))
    };

    match build() {
        Ok((stream, src_rate)) => {
            tracing::info!("capture started (device rate {src_rate} Hz)");
            let _ = ready_tx.send(Ok(()));
            // Hold the stream alive until stop is requested or all receivers
            // are gone.
            let _ = stop_rx.recv();
            drop(stream);
            tracing::info!("capture stopped");
        }
        Err(e) => {
            let _ = ready_tx.send(Err(e));
        }
    }
}

fn mix_mono(interleaved: &[f32], channels: usize, out: &mut Vec<f32>) {
    out.clear();
    if channels <= 1 {
        out.extend_from_slice(interleaved);
        return;
    }
    for frame in interleaved.chunks_exact(channels) {
        out.push(frame.iter().sum::<f32>() / channels as f32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_halves_48k_to_16k_length() {
        let mut rs = LinearResampler::new(48_000, 16_000);
        let input: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.001).sin()).collect();
        let mut out = Vec::new();
        rs.process(&input, &mut out);
        // ~1 second of audio -> ~16000 samples (allow small edge loss)
        assert!((out.len() as i64 - 16_000).abs() < 10, "got {}", out.len());
    }

    #[test]
    fn resampler_streaming_matches_oneshot() {
        let input: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.01).sin()).collect();
        let mut one = Vec::new();
        LinearResampler::new(48_000, 16_000).process(&input, &mut one);

        let mut streamed = Vec::new();
        let mut rs = LinearResampler::new(48_000, 16_000);
        for chunk in input.chunks(311) {
            rs.process(chunk, &mut streamed);
        }
        assert_eq!(one.len(), streamed.len());
        for (a, b) in one.iter().zip(streamed.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn passthrough_at_target_rate() {
        let input: Vec<f32> = vec![0.5; 1600];
        let out = resample_to_target(&input, TARGET_SAMPLE_RATE);
        assert_eq!(out.len(), 1600);
        assert!(out.iter().all(|s| (*s - (0.5 * i16::MAX as f32) as i16).abs() <= 1));
    }
}
