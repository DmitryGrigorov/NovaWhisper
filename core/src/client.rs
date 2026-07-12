//! WebSocket streaming client: one connection per utterance.

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{timeout, Duration};
use tokio_tungstenite::tungstenite::Message;

use crate::protocol::{ClientMessage, ServerMessage};

/// Events surfaced to the UI layer while streaming.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    Ready,
    Partial(String),
    Final {
        text: String,
        raw_text: String,
        duration_ms: u64,
    },
    Error(String),
    Closed,
}

fn pcm_to_bytes(chunk: &[i16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(chunk.len() * 2);
    for s in chunk {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    bytes
}

/// Stream one utterance to the gateway.
///
/// Audio chunks arrive on `audio_rx` (16 kHz mono i16). The utterance ends
/// when `stop_rx` fires or `audio_rx` closes; the function then waits for the
/// server's `final` message (up to `final_timeout`) and returns.
pub async fn stream_utterance(
    url: &str,
    start: ClientMessage,
    mut audio_rx: mpsc::Receiver<Vec<i16>>,
    stop_rx: oneshot::Receiver<()>,
    events_tx: mpsc::Sender<StreamEvent>,
) -> Result<()> {
    let final_timeout = Duration::from_secs(15);
    // A dropped stop sender means "no explicit stop": the utterance then ends
    // when the audio channel closes.
    let stop_fut = async {
        if stop_rx.await.is_err() {
            std::future::pending::<()>().await;
        }
    };
    tokio::pin!(stop_fut);
    let (ws, _resp) = tokio_tungstenite::connect_async(url)
        .await
        .with_context(|| format!("failed to connect to gateway at {url}"))?;
    let (mut sink, mut source) = ws.split();

    sink.send(Message::Text(serde_json::to_string(&start)?))
        .await
        .context("failed to send start message")?;

    let emit = |e: StreamEvent| {
        let tx = events_tx.clone();
        async move {
            let _ = tx.send(e).await;
        }
    };

    // Streaming phase: forward audio, surface partials, watch for stop.
    let mut got_final = false;
    loop {
        tokio::select! {
            biased;
            _ = &mut stop_fut => {
                break;
            }
            chunk = audio_rx.recv() => {
                match chunk {
                    Some(chunk) => {
                        sink.send(Message::Binary(pcm_to_bytes(&chunk)))
                            .await
                            .context("failed to send audio frame")?;
                    }
                    None => break, // capture ended
                }
            }
            msg = source.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if handle_server_text(&text, &events_tx).await? {
                            got_final = true;
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        emit(StreamEvent::Closed).await;
                        return Ok(());
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return Err(e).context("websocket error"),
                }
            }
        }
    }

    if !got_final {
        sink.send(Message::Text(serde_json::to_string(&ClientMessage::Stop)?))
            .await
            .context("failed to send stop message")?;

        // Drain phase: wait for the final transcript.
        let drain = async {
            while let Some(msg) = source.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        if handle_server_text(&text, &events_tx).await? {
                            return Ok::<_, anyhow::Error>(true);
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Ok(_) => {}
                    Err(e) => return Err(e).context("websocket error while draining"),
                }
            }
            Ok(false)
        };
        match timeout(final_timeout, drain).await {
            Ok(Ok(true)) => {}
            Ok(Ok(false)) => {
                emit(StreamEvent::Error(
                    "connection closed before final transcript".into(),
                ))
                .await
            }
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                emit(StreamEvent::Error(
                    "timed out waiting for final transcript".into(),
                ))
                .await
            }
        }
    }

    let _ = sink.send(Message::Close(None)).await;
    emit(StreamEvent::Closed).await;
    Ok(())
}

/// Parse a server text frame, emit the matching event; returns true on `final`.
async fn handle_server_text(text: &str, events_tx: &mpsc::Sender<StreamEvent>) -> Result<bool> {
    let msg: ServerMessage =
        serde_json::from_str(text).with_context(|| format!("bad server message: {text}"))?;
    let (event, is_final) = match msg {
        ServerMessage::Ready => (StreamEvent::Ready, false),
        ServerMessage::Partial { text } => (StreamEvent::Partial(text), false),
        ServerMessage::Final {
            text,
            raw_text,
            duration_ms,
        } => (
            StreamEvent::Final {
                text,
                raw_text,
                duration_ms,
            },
            true,
        ),
        ServerMessage::Error { message } => (StreamEvent::Error(message), false),
    };
    let _ = events_tx.send(event).await;
    Ok(is_final)
}
