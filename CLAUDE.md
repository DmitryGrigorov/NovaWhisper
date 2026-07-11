# Whispr — agent guide

Cross-platform voice-to-text dictation (Wispr Flow alternative): Tauri 2 + Rust
desktop app streams mic audio to a FastAPI gateway (WebSocket), which runs STT
(local Whisper / Deepgram / offline mock) and polishes the transcript, then the
app types the result into the focused application. The app starts/stops the
gateway itself (`apps/desktop/src-tauri/src/gateway.rs`): bundled sidecar or
repo `.venv`; an externally started gateway is used but never killed.

**Read before changing code:**

- `docs/STRUCTURE.md` — file map, runtime picture, **invariants & sync points**
  (protocol lives in 3 places; event contract; config↔UI field names).
- `docs/SKILLS.md` — build/run/test commands, E2E verification without a mic,
  recipes (add STT provider, change protocol, add setting, native insertion).
- `docs/ARCHITECTURE.md` — stack rationale, P0–P2 roadmap, risk register.

**Quick commands:**

```sh
cargo build --workspace && cargo test --workspace
cd server/gateway && ./.venv/bin/python -m pytest        # venv already in repo dir
./.venv/bin/uvicorn app.main:app --port 8765             # gateway (mock STT by default)
cargo run -p whispr-desktop                              # needs a display
cargo check -p whispr-core -p whispr-insert --target x86_64-pc-windows-gnu  # Windows compat
```

**Hard rules:**

1. Protocol changes touch `shared/protocol.md` + `core/src/protocol.rs` +
   `server/gateway/app/main.py` + `tests/test_stream.py` — all four, same PR.
2. Every STT provider pushes exactly one `None` sentinel to `partial_queue`.
3. `polish()` must never raise — LLM path falls back to `apply_rules`.
4. cpal `Stream` is `!Send`; it stays on the capture thread in `core/src/audio.rs`.
5. New `AppConfig` fields need defaults (`#[serde(default)]`) and a matching
   input id in `apps/desktop/ui/index.html`.
6. Before done: both test suites green + the no-mic E2E check in SKILLS.md.

Primary target machine: Windows 11 + NVIDIA GPU (local GPU Whisper,
`large-v3` = `Systran/faster-whisper-large-v3`). Dev container is Linux;
GUI verified via xvfb.
