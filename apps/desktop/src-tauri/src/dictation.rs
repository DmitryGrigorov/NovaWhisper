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
        Ok(false)
    } else {
        match start(app) {
            Ok(session) => {
                *guard = Some(session);
                emit(app, json!({"kind": "status", "recording": true}));
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
        polish: PolishOptions { mode: cfg.polish_mode.clone() },
    };

    show_hud(app);

    // Streaming task: audio out, transcripts in.
    let url = cfg.gateway_url.clone();
    let stream_app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = stream_utterance(&url, start_msg, audio_rx, stop_rx, events_tx).await {
            emit(&stream_app, json!({"kind": "error", "message": e.to_string()}));
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
                StreamEvent::Final { text, raw_text, duration_ms } => {
                    let final_text = snippets.expand(&text);
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
                    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                    hide_hud(&event_app);
                }
                StreamEvent::Error(message) => {
                    emit(&event_app, json!({"kind": "error", "message": message}));
                }
                StreamEvent::Closed => {}
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

    Ok(ActiveSession { stop_tx: Some(stop_tx), capture })
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
