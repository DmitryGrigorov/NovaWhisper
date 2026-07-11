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
}
