//! Whispr core: platform-independent audio pipeline, streaming client and
//! text intelligence shared by all Whispr front-ends (desktop, mobile via FFI).

pub mod audio;
pub mod client;
pub mod config;
pub mod dictionary;
pub mod protocol;
pub mod vad;

pub use audio::{AudioCapture, CaptureHandle, TARGET_SAMPLE_RATE};
pub use client::{stream_utterance, StreamEvent};
pub use config::AppConfig;
pub use dictionary::SnippetEngine;
