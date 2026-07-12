//! Wire types for the Whispr streaming protocol (see shared/protocol.md).

use serde::{Deserialize, Serialize};

pub const AUDIO_FORMAT: &str = "pcm_s16le";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSpec {
    pub format: String,
    pub sample_rate: u32,
    pub channels: u16,
}

impl Default for AudioSpec {
    fn default() -> Self {
        Self {
            format: AUDIO_FORMAT.to_string(),
            sample_rate: crate::audio::TARGET_SAMPLE_RATE,
            channels: 1,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolishOptions {
    /// "none" | "fillers" | "full"
    pub mode: String,
}

impl Default for PolishOptions {
    fn default() -> Self {
        Self {
            mode: "fillers".to_string(),
        }
    }
}

/// Client -> server control messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Start {
        audio: AudioSpec,
        language: String,
        #[serde(default)]
        context: SessionContext,
        #[serde(default)]
        dictionary: Vec<String>,
        #[serde(default)]
        polish: PolishOptions,
    },
    Stop,
}

/// Server -> client messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Ready,
    Partial {
        text: String,
    },
    Final {
        text: String,
        raw_text: String,
        #[serde(default)]
        duration_ms: u64,
    },
    Error {
        message: String,
    },
}
