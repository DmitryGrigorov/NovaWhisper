//! Persistent app configuration (config_dir/whispr/config.json).

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::dictionary::Snippet;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Gateway websocket endpoint.
    pub gateway_url: String,
    /// Global hotkey (Tauri global-shortcut syntax).
    pub hotkey: String,
    /// BCP-47 language code or "auto".
    pub language: String,
    /// "none" | "fillers" | "full".
    pub polish_mode: String,
    /// "auto" | "paste" | "type".
    pub insert_method: String,
    /// Audio chunk size sent over the wire, in ms.
    pub chunk_ms: u32,
    /// Input device name, or `None` to follow the system default.
    pub microphone: Option<String>,
    /// End the utterance automatically after trailing silence.
    pub auto_stop: bool,
    /// Show the recording HUD. Disabled by default for unobtrusive dictation.
    pub show_hud: bool,
    /// Hide the HUD a few seconds after a final transcript is received.
    pub autohide_hud: bool,
    /// Start and stop the local gateway together with the app (loopback
    /// gateway_url only). An already-running gateway is detected and left alone.
    pub manage_gateway: bool,
    /// STT provider for the managed gateway: "auto" | "whisper_local" |
    /// "deepgram" | "mock". "auto" lets the gateway pick the best available.
    pub stt_provider: String,
    /// faster-whisper model name or CTranslate2 dir for the managed gateway.
    pub whisper_model: String,
    /// Whisper device: "auto" | "cuda" | "cpu".
    pub whisper_device: String,
    /// Whisper compute type: "default" | "float16" | "int8_float16" | "int8".
    pub whisper_compute: String,
    pub dictionary: Vec<String>,
    pub snippets: Vec<Snippet>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            gateway_url: "ws://127.0.0.1:8765/v1/stream".into(),
            hotkey: "ctrl+shift+space".into(),
            language: "auto".into(),
            polish_mode: "fillers".into(),
            insert_method: "auto".into(),
            chunk_ms: 100,
            microphone: None,
            auto_stop: false,
            show_hud: false,
            autohide_hud: true,
            manage_gateway: true,
            stt_provider: "auto".into(),
            whisper_model: "large-v3".into(),
            whisper_device: "auto".into(),
            whisper_compute: "default".into(),
            dictionary: Vec::new(),
            snippets: Vec::new(),
        }
    }
}

impl AppConfig {
    pub fn path() -> Result<PathBuf> {
        let dir = dirs::config_dir()
            .context("no config directory on this platform")?
            .join("whispr");
        Ok(dir.join("config.json"))
    }

    /// Load the config, creating it with defaults on first run.
    pub fn load() -> AppConfig {
        let Ok(path) = Self::path() else {
            return AppConfig::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
                tracing::warn!("invalid config at {}: {e}; using defaults", path.display());
                AppConfig::default()
            }),
            Err(_) => {
                let cfg = AppConfig::default();
                let _ = cfg.save();
                cfg
            }
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_string_pretty(self)?)
            .with_context(|| format!("failed to write {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_config_uses_system_default_microphone() {
        let config: AppConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(config.microphone, None);
    }

    #[test]
    fn old_config_gets_gateway_management_defaults() {
        let config: AppConfig = serde_json::from_str("{}").unwrap();
        assert!(!config.show_hud);
        assert!(config.autohide_hud);
        assert!(config.manage_gateway);
        assert_eq!(config.stt_provider, "auto");
        assert_eq!(config.whisper_model, "large-v3");
        assert_eq!(config.whisper_device, "auto");
        assert_eq!(config.whisper_compute, "default");
    }

    #[test]
    fn language_insertion_and_hud_settings_round_trip() {
        let config = AppConfig {
            language: "ru".into(),
            insert_method: "paste".into(),
            show_hud: true,
            autohide_hud: false,
            ..AppConfig::default()
        };

        let raw = serde_json::to_string(&config).unwrap();
        let restored: AppConfig = serde_json::from_str(&raw).unwrap();

        assert_eq!(restored.language, "ru");
        assert_eq!(restored.insert_method, "paste");
        assert!(restored.show_hud);
        assert!(!restored.autohide_hud);
    }
}
