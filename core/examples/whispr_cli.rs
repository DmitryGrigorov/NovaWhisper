//! CLI dogfood harness: stream a WAV file (or the microphone) to the gateway
//! and print partial/final transcripts.
//!
//!   cargo run -p whispr-core --example whispr-cli -- --wav path/to/audio.wav
//!   cargo run -p whispr-core --example whispr-cli -- --mic --seconds 5

use anyhow::{bail, Context, Result};
use clap::Parser;
use tokio::sync::{mpsc, oneshot};

use whispr_core::audio::{resample_to_target, AudioCapture, TARGET_SAMPLE_RATE};
use whispr_core::client::{stream_utterance, StreamEvent};
use whispr_core::protocol::{AudioSpec, ClientMessage, PolishOptions, SessionContext};

#[derive(Parser)]
struct Args {
    /// Gateway websocket URL.
    #[arg(long, default_value = "ws://127.0.0.1:8765/v1/stream")]
    url: String,
    /// WAV file to stream (any rate, mono or stereo, 16-bit or float).
    #[arg(long)]
    wav: Option<String>,
    /// Capture from the default microphone instead.
    #[arg(long)]
    mic: bool,
    /// Seconds to record in --mic mode.
    #[arg(long, default_value_t = 5)]
    seconds: u64,
    /// Language code or "auto".
    #[arg(long, default_value = "auto")]
    language: String,
    /// Polish mode: none | fillers | full.
    #[arg(long, default_value = "fillers")]
    polish: String,
    /// Pace WAV streaming in real time (default streams at 10x).
    #[arg(long)]
    realtime: bool,
}

fn load_wav_as_16k_mono(path: &str) -> Result<Vec<i16>> {
    let mut reader =
        hound::WavReader::open(path).with_context(|| format!("cannot open wav file {path}"))?;
    let spec = reader.spec();
    let mono: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            let samples: Vec<f32> = reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / max))
                .collect::<std::result::Result<_, _>>()?;
            mix_channels(&samples, spec.channels as usize)
        }
        hound::SampleFormat::Float => {
            let samples: Vec<f32> = reader
                .samples::<f32>()
                .collect::<std::result::Result<_, _>>()?;
            mix_channels(&samples, spec.channels as usize)
        }
    };
    Ok(resample_to_target(&mono, spec.sample_rate))
}

fn mix_channels(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(channels)
        .map(|f| f.iter().sum::<f32>() / channels as f32)
        .collect()
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    let start = ClientMessage::Start {
        audio: AudioSpec::default(),
        language: args.language.clone(),
        context: SessionContext {
            app: Some("whispr-cli".into()),
            style: None,
        },
        dictionary: vec![],
        polish: PolishOptions {
            mode: args.polish.clone(),
        },
    };

    let (events_tx, mut events_rx) = mpsc::channel::<StreamEvent>(64);
    let (stop_tx, stop_rx) = oneshot::channel::<()>();

    let printer = tokio::spawn(async move {
        while let Some(event) = events_rx.recv().await {
            match event {
                StreamEvent::Ready => eprintln!("[ready]"),
                StreamEvent::Partial(t) => eprintln!("[partial] {t}"),
                StreamEvent::Final {
                    text,
                    raw_text,
                    duration_ms,
                } => {
                    eprintln!("[final raw]      {raw_text}");
                    eprintln!("[final polished] {text}  ({duration_ms} ms)");
                    println!("{text}");
                }
                StreamEvent::Error(m) => eprintln!("[error] {m}"),
                StreamEvent::Closed => eprintln!("[closed]"),
            }
        }
    });

    if let Some(wav) = &args.wav {
        let samples = load_wav_as_16k_mono(wav)?;
        eprintln!(
            "streaming {:.1}s of audio from {wav}",
            samples.len() as f32 / TARGET_SAMPLE_RATE as f32
        );
        let (audio_tx, audio_rx) = mpsc::channel::<Vec<i16>>(128);
        let chunk = (TARGET_SAMPLE_RATE / 10) as usize; // 100 ms
        let pace = if args.realtime { 100 } else { 10 };
        let feeder = tokio::spawn(async move {
            for c in samples.chunks(chunk) {
                if audio_tx.send(c.to_vec()).await.is_err() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(pace)).await;
            }
            // Dropping audio_tx ends the utterance (like releasing the hotkey).
        });
        drop(stop_tx);
        stream_utterance(&args.url, start, audio_rx, stop_rx, events_tx).await?;
        feeder.await?;
    } else if args.mic {
        let mut capture = AudioCapture::start(100, None)?;
        eprintln!("recording from microphone for {}s...", args.seconds);
        let audio_rx = std::mem::replace(&mut capture.audio_rx, mpsc::channel(1).1);
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(args.seconds)).await;
            let _ = stop_tx.send(());
        });
        stream_utterance(&args.url, start, audio_rx, stop_rx, events_tx).await?;
        capture.stop();
    } else {
        bail!("pass --wav <file> or --mic");
    }

    printer.await?;
    Ok(())
}
