# Whispr Streaming Protocol v1

Transport: WebSocket (`wss://` in production, TLS 1.3). One connection per utterance
("session"). Text frames carry JSON control messages; binary frames carry raw audio.

## Audio format

- PCM signed 16-bit little-endian (`pcm_s16le`), 16 000 Hz, mono.
- Binary frames of ~100–200 ms (3 200–6 400 bytes). Opus is a planned optimization.

## Client → Server

### `start` (first message, JSON text frame)

```json
{
  "type": "start",
  "audio": { "format": "pcm_s16le", "sample_rate": 16000, "channels": 1 },
  "language": "auto",
  "context": {
    "app": "com.example.editor",
    "style": "default"
  },
  "dictionary": ["Whispr", "Grigorov"],
  "polish": { "mode": "fillers" }
}
```

`polish.mode`: `"none"` | `"fillers"` (rule-based, P0) | `"full"` (LLM, P1).

### audio (binary frames)

Raw PCM chunks as described above, sent while the user speaks.

### `stop` (JSON text frame)

```json
{ "type": "stop" }
```

Signals end of utterance. The server flushes the STT provider, runs polish, sends
`final`, then closes.

## Server → Client (JSON text frames)

```json
{ "type": "ready" }
{ "type": "partial", "text": "hello wor" }
{ "type": "final", "text": "Hello world.", "raw_text": "um hello world", "duration_ms": 1840 }
{ "type": "error", "message": "..." }
```

- `partial` may be sent any number of times (monotonically growing best-guess text).
- Exactly one `final` is sent after `stop` (or after server-side end-of-speech timeout).
