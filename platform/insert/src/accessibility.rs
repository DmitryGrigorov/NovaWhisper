//! Tier-1 native accessibility insertion, per platform.
//!
//! Contract: insert `text` at the caret of the focused editable element and
//! return Ok, or return an error to fall through to the paste tier. These are
//! the P0->P1 hardening points per docs/ARCHITECTURE.md §3:
//!
//! - macOS: AXUIElementSetAttributeValue(kAXSelectedTextAttribute) on the
//!   focused element (requires AXIsProcessTrusted).
//! - Windows: IUIAutomation focused element -> ValuePattern.SetValue /
//!   TextPattern selection insertion.
//! - Linux: AT-SPI2 org.a11y.atspi EditableText.InsertText on the focused
//!   object (X11 & Wayland; coverage varies by toolkit).

use anyhow::{bail, Result};

#[cfg(target_os = "linux")]
pub fn insert(_text: &str) -> Result<()> {
    // AT-SPI EditableText insertion is planned (tracked in ARCHITECTURE.md
    // R-2/R-3); the paste tier is the reliable default on Wayland today.
    bail!("AT-SPI insertion not yet implemented on Linux")
}

#[cfg(target_os = "macos")]
pub fn insert(_text: &str) -> Result<()> {
    bail!("AX insertion not yet implemented on macOS")
}

#[cfg(target_os = "windows")]
pub fn insert(_text: &str) -> Result<()> {
    bail!("UIA insertion not yet implemented on Windows")
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub fn insert(_text: &str) -> Result<()> {
    bail!("no accessibility backend for this platform")
}
