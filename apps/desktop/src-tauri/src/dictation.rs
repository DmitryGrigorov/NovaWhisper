//! Dictation session lifecycle: hotkey toggle -> capture -> stream -> insert.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

static HUD_POSITIONED: AtomicBool = AtomicBool::new(false);
use tokio::sync::{mpsc, oneshot};

use whispr_core::audio::{AudioCapture, CaptureHandle};
use whispr_core::client::{stream_utterance, StreamEvent};
use whispr_core::protocol::{AudioSpec, ClientMessage, PolishOptions, SessionContext};
use whispr_core::{AppConfig, SnippetEngine};
use whispr_insert::InsertMethod;

pub const EVENT: &str = "whispr://event";

/// Run input synthesis on the application main queue. On macOS Enigo reads
/// the current HIToolbox keyboard layout, whose API traps when called from a
/// Tokio worker thread.
pub async fn insert_text(
    app: &AppHandle,
    text: String,
    method: InsertMethod,
) -> Result<whispr_insert::InsertedVia, String> {
    let (tx, rx) = oneshot::channel();
    app.run_on_main_thread(move || {
        let result = whispr_insert::insert_text(&text, method).map_err(|e| e.to_string());
        let _ = tx.send(result);
    })
    .map_err(|e| format!("cannot schedule insertion: {e}"))?;

    rx.await
        .map_err(|_| "insertion ended without returning a result".to_string())?
}

#[derive(Default)]
pub struct DictationState(pub Mutex<Option<ActiveSession>>);

/// Most recent non-empty final transcript of this app run, for the
/// tray "Copy Latest Transcript" action and its global hotkey.
#[derive(Default)]
pub struct LatestTranscript(pub Mutex<String>);

/// Copy the latest final transcript to the system clipboard.
pub fn copy_latest(app: &AppHandle) {
    let text = app
        .state::<LatestTranscript>()
        .0
        .lock()
        .map(|t| t.clone())
        .unwrap_or_default();
    if text.is_empty() {
        emit(
            app,
            json!({"kind": "error", "message": "No transcript to copy yet."}),
        );
        return;
    }
    match whispr_insert::copy_to_clipboard(&text) {
        Ok(()) => emit(app, json!({"kind": "copied", "text": text})),
        Err(e) => emit(
            app,
            json!({"kind": "error", "message": format!("copy failed: {e}")}),
        ),
    }
}

/// Dictation lifecycle as reflected in the tray icon, so the user can tell
/// when a recording started and when the transcript has landed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TrayStatus {
    Idle,
    Recording,
    Transcribing,
}

/// Swap the tray icon and tooltip to match the dictation state: red dot while
/// recording, amber dot while a transcript is pending, app icon when idle.
pub fn set_tray_status(app: &AppHandle, status: TrayStatus) {
    let Some(tray) = app.tray_by_id("whispr-tray") else {
        return;
    };
    let (icon, tooltip) = match status {
        TrayStatus::Idle => (
            app.default_window_icon().cloned(),
            "Whispr — voice to text anywhere",
        ),
        TrayStatus::Recording => (
            Some(dot_icon([0xE5, 0x3E, 0x3E])),
            "Whispr — recording… (press the hotkey to stop)",
        ),
        TrayStatus::Transcribing => (
            Some(dot_icon([0xF0, 0xA8, 0x2E])),
            "Whispr — transcribing…",
        ),
    };
    if let Some(icon) = icon {
        let _ = tray.set_icon(Some(icon));
    }
    let _ = tray.set_tooltip(Some(tooltip));
}

/// 32×32 filled-circle "state light" rendered at runtime, so no icon assets
/// are needed per state.
fn dot_icon(rgb: [u8; 3]) -> tauri::image::Image<'static> {
    const SIZE: usize = 32;
    let mut rgba = vec![0u8; SIZE * SIZE * 4];
    let center = (SIZE as f32 - 1.0) / 2.0;
    let radius = SIZE as f32 * 0.42;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            if dx * dx + dy * dy <= radius * radius {
                let i = (y * SIZE + x) * 4;
                rgba[i..i + 3].copy_from_slice(&rgb);
                rgba[i + 3] = 0xFF;
            }
        }
    }
    tauri::image::Image::new_owned(rgba, SIZE as u32, SIZE as u32)
}

pub struct ActiveSession {
    stop_tx: Option<oneshot::Sender<()>>,
    capture: CaptureHandle,
}

fn emit(app: &AppHandle, payload: serde_json::Value) {
    let _ = app.emit(EVENT, payload);
}

/// Toggle dictation on/off. Returns true when now recording.
pub fn toggle(app: &AppHandle) -> Result<bool, String> {
    let state = app.state::<DictationState>();
    let mut guard = state.0.lock().unwrap();
    if let Some(mut session) = guard.take() {
        if let Some(tx) = session.stop_tx.take() {
            let _ = tx.send(());
        }
        session.capture.stop();
        emit(app, json!({"kind": "status", "recording": false}));
        // The final transcript is still on its way; show that in the tray.
        set_tray_status(app, TrayStatus::Transcribing);
        Ok(false)
    } else {
        match start(app) {
            Ok(session) => {
                *guard = Some(session);
                emit(app, json!({"kind": "status", "recording": true}));
                set_tray_status(app, TrayStatus::Recording);
                Ok(true)
            }
            Err(e) => {
                emit(app, json!({"kind": "error", "message": e.clone()}));
                Err(e)
            }
        }
    }
}

fn start(app: &AppHandle) -> Result<ActiveSession, String> {
    let cfg = AppConfig::load();

    let mut capture = AudioCapture::start(cfg.chunk_ms, cfg.microphone.as_deref())
        .map_err(|e| format!("microphone unavailable: {e}"))?;
    let audio_rx = std::mem::replace(&mut capture.audio_rx, mpsc::channel(1).1);
    let level = capture.level_probe();

    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let (events_tx, mut events_rx) = mpsc::channel::<StreamEvent>(64);

    let start_msg = ClientMessage::Start {
        audio: AudioSpec::default(),
        language: cfg.language.clone(),
        context: SessionContext::default(),
        dictionary: cfg.dictionary.clone(),
        polish: PolishOptions {
            mode: cfg.polish_mode.clone(),
        },
    };

    if cfg.show_hud {
        show_hud(app);
    }

    // Streaming task: audio out, transcripts in.
    let url = cfg.gateway_url.clone();
    let stream_app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = stream_utterance(&url, start_msg, audio_rx, stop_rx, events_tx).await {
            emit(
                &stream_app,
                json!({"kind": "error", "message": e.to_string()}),
            );
            set_tray_status(&stream_app, TrayStatus::Idle);
        }
    });

    // Event task: HUD updates + final insertion.
    let snippets = SnippetEngine::new(cfg.snippets.clone());
    let insert_method = InsertMethod::parse(&cfg.insert_method);
    let event_app = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = events_rx.recv().await {
            match event {
                StreamEvent::Ready => {
                    emit(&event_app, json!({"kind": "ready"}));
                }
                StreamEvent::Partial(text) => {
                    emit(&event_app, json!({"kind": "partial", "text": text}));
                }
                StreamEvent::Final {
                    text,
                    raw_text,
                    duration_ms,
                } => {
                    let final_text = snippets.expand(&text);
                    if !final_text.is_empty() {
                        if let Ok(mut latest) = event_app.state::<LatestTranscript>().0.lock() {
                            *latest = final_text.clone();
                        }
                    }
                    emit(
                        &event_app,
                        json!({
                            "kind": "final",
                            "text": final_text,
                            "raw_text": raw_text,
                            "duration_ms": duration_ms
                        }),
                    );
                    finish_session(&event_app);
                    if !final_text.is_empty() {
                        match insert_text(&event_app, final_text.clone(), insert_method).await {
                            Ok(via) => emit(
                                &event_app,
                                json!({"kind": "inserted", "via": format!("{via:?}")}),
                            ),
                            Err(e) => emit(
                                &event_app,
                                json!({"kind": "error", "message": format!("insertion failed: {e}")}),
                            ),
                        }
                    }
                    if cfg.show_hud && cfg.autohide_hud {
                        // Leave the completed transcript visible long enough
                        // for the user to copy it from the HUD.
                        tokio::time::sleep(std::time::Duration::from_millis(5000)).await;
                        hide_hud(&event_app);
                    }
                }
                StreamEvent::Error(message) => {
                    emit(&event_app, json!({"kind": "error", "message": message}));
                    set_tray_status(&event_app, TrayStatus::Idle);
                }
                StreamEvent::Closed => {
                    set_tray_status(&event_app, TrayStatus::Idle);
                }
            }
        }
    });

    // Mic level meter for the HUD, while the session is active.
    let level_app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(80)).await;
            let active = level_app
                .state::<DictationState>()
                .0
                .lock()
                .map(|g| g.is_some())
                .unwrap_or(false);
            if !active {
                break;
            }
            emit(&level_app, json!({"kind": "level", "value": level.level()}));
        }
    });

    Ok(ActiveSession {
        stop_tx: Some(stop_tx),
        capture,
    })
}

/// Drop any leftover session state (e.g. server ended the utterance).
fn finish_session(app: &AppHandle) {
    let state = app.state::<DictationState>();
    if let Ok(mut guard) = state.0.lock() {
        if let Some(mut session) = guard.take() {
            if let Some(tx) = session.stop_tx.take() {
                let _ = tx.send(());
            }
            session.capture.stop();
        }
    }
    emit(app, json!({"kind": "status", "recording": false}));
    set_tray_status(app, TrayStatus::Idle);
}

pub fn show_hud(app: &AppHandle) {
    if let Some(hud) = app.get_webview_window("hud") {
        if !HUD_POSITIONED.swap(true, Ordering::Relaxed) {
            position_hud(app, &hud);
        }
        let _ = hud.show();
    }
}

pub fn hide_hud(app: &AppHandle) {
    if let Some(hud) = app.get_webview_window("hud") {
        let _ = hud.hide();
    }
}

fn position_hud(app: &AppHandle, hud: &tauri::WebviewWindow) {
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let screen = monitor.size();
        if let Ok(win) = hud.outer_size() {
            let x = (screen.width.saturating_sub(win.width)) / 2;
            let y = screen.height.saturating_sub(win.height + 80);
            let _ = hud.set_position(tauri::PhysicalPosition::new(x as i32, y as i32));
        }
    }
}
