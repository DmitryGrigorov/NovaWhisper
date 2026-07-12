//! Cross-application text insertion with tiered fallbacks.
//!
//! Tier 1 — native accessibility API at the caret (AX / UIA / AT-SPI).
//!          Per-OS implementations land behind [`accessibility`]; each returns
//!          `Unsupported` until implemented, falling through to tier 2.
//! Tier 2 — clipboard paste: save clipboard, set text, synthesize
//!          Cmd/Ctrl+V, restore clipboard. The workhorse in practice.
//! Tier 3 — synthetic keystrokes typing the text (slow, last resort).

use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result};
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

pub mod accessibility;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertMethod {
    /// Try accessibility, then paste, then typing.
    Auto,
    Accessibility,
    Paste,
    Type,
}

impl InsertMethod {
    pub fn parse(s: &str) -> Self {
        match s {
            "accessibility" => Self::Accessibility,
            "paste" => Self::Paste,
            "type" => Self::Type,
            _ => Self::Auto,
        }
    }
}

/// Which tier actually performed the insertion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertedVia {
    Accessibility,
    Paste,
    Type,
}

/// Insert `text` into the currently focused application.
pub fn insert_text(text: &str, method: InsertMethod) -> Result<InsertedVia> {
    if text.is_empty() {
        return Ok(InsertedVia::Type);
    }
    match method {
        InsertMethod::Accessibility => {
            accessibility::insert(text).map(|_| InsertedVia::Accessibility)
        }
        InsertMethod::Paste => paste_insert(text).map(|_| InsertedVia::Paste),
        InsertMethod::Type => type_insert(text).map(|_| InsertedVia::Type),
        InsertMethod::Auto => {
            match accessibility::insert(text) {
                Ok(()) => return Ok(InsertedVia::Accessibility),
                Err(e) => tracing::debug!("accessibility insert unavailable: {e}"),
            }
            match paste_insert(text) {
                Ok(()) => return Ok(InsertedVia::Paste),
                Err(e) => tracing::warn!("paste insert failed: {e}; falling back to typing"),
            }
            type_insert(text).map(|_| InsertedVia::Type)
        }
    }
}

/// Tier 2: clipboard set + synthetic paste chord + clipboard restore.
fn paste_insert(text: &str) -> Result<()> {
    let mut clipboard = arboard::Clipboard::new().context("cannot open clipboard")?;
    let saved = clipboard.get_text().ok();

    clipboard
        .set_text(text.to_string())
        .context("cannot write clipboard")?;
    // Give the focused app's clipboard watcher a beat before pasting.
    sleep(Duration::from_millis(60));

    let mut enigo = Enigo::new(&Settings::default()).context("cannot create input synthesizer")?;
    let modifier = if cfg!(target_os = "macos") {
        Key::Meta
    } else {
        Key::Control
    };
    enigo
        .key(modifier, Direction::Press)
        .context("modifier press failed")?;
    enigo
        .key(Key::Unicode('v'), Direction::Click)
        .context("paste key failed")?;
    enigo
        .key(modifier, Direction::Release)
        .context("modifier release failed")?;

    // Let the paste land before restoring the clipboard.
    sleep(Duration::from_millis(150));
    if let Some(prev) = saved {
        let _ = clipboard.set_text(prev);
    }
    Ok(())
}

/// Tier 3: type the text as synthetic keystrokes.
fn type_insert(text: &str) -> Result<()> {
    let mut enigo = Enigo::new(&Settings::default()).context("cannot create input synthesizer")?;
    enigo.text(text).context("synthetic typing failed")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insertion_method_values_match_settings_ui() {
        assert_eq!(InsertMethod::parse("auto"), InsertMethod::Auto);
        assert_eq!(InsertMethod::parse("paste"), InsertMethod::Paste);
        assert_eq!(InsertMethod::parse("type"), InsertMethod::Type);
        assert_eq!(
            InsertMethod::parse("accessibility"),
            InsertMethod::Accessibility
        );
    }

    #[test]
    fn unknown_insertion_method_falls_back_safely() {
        assert_eq!(InsertMethod::parse("invalid"), InsertMethod::Auto);
    }

    #[test]
    fn empty_text_does_not_touch_clipboard_or_keyboard() {
        assert_eq!(
            insert_text("", InsertMethod::Auto).unwrap(),
            InsertedVia::Type
        );
    }
}
