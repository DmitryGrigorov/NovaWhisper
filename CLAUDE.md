# NovaWhisper — agent guide

Cross-platform voice dictation: a Tauri 2 + Rust desktop app captures mic audio
and streams it to a FastAPI gateway over WebSocket. The gateway runs STT
(mock / local Whisper / Deepgram), polishes the transcript, and the desktop app
inserts it into the focused application. The app manages the gateway itself for
source and packaged builds; an externally started gateway is detected and reused.

Read before changing code:

- `docs/STRUCTURE.md` — file map, runtime picture, invariants, and sync points
  (wire protocol in three places; event contract; config↔UI field names).
- `docs/SKILLS.md` — build/run/test commands, no-mic E2E verification, and task
  recipes for providers, protocol changes, settings, insertion, packaging, and VAD.
- `docs/ARCHITECTURE.md` — stack rationale, roadmap, and risk register.
- `README.md` / `README.ru.md` — installation, setup, and supported environment details.

Quick commands:

```sh
cargo build --workspace && cargo test --workspace
cd server/gateway && ./.venv/bin/python -m pytest
./.venv/bin/uvicorn app.main:app --host 127.0.0.1 --port 8765
cargo run -p whispr-desktop
cargo check -p whispr-core -p whispr-insert --target x86_64-pc-windows-gnu
```

Hard rules:

1. Protocol changes touch `shared/protocol.md`, `core/src/protocol.rs`,
   `server/gateway/app/main.py`, and `server/gateway/tests/test_stream.py` —
   all four, same PR.
2. Every STT provider pushes exactly one `None` sentinel to `partial_queue`.
3. `polish()` must never raise; the LLM path falls back to `apply_rules` on any
   failure.
4. The `cpal` `Stream` is `!Send`; keep it on the dedicated capture thread in
   `core/src/audio.rs`.
5. New `AppConfig` fields need `#[serde(default)]` defaults and matching input
   ids in `apps/desktop/ui/index.html`.
6. Managed gateway lifecycle lives in `apps/desktop/src-tauri/src/gateway.rs`;
   never kill an externally started gateway.
7. Before considering work done: the Rust tests, gateway tests, and the no-mic
   E2E check in `docs/SKILLS.md` must all pass.

Primary target machine: Windows 11 + NVIDIA GPU with local Whisper (typically
`large-v3` / `Systran/faster-whisper-large-v3`). The dev container is Linux;
GUI checks are verified via `xvfb`.
