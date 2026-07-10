# Skills — task recipes for working on Whispr

*Step-by-step playbooks for the changes this repo most often needs. File
locations and invariants are in [STRUCTURE.md](STRUCTURE.md); design rationale
in [ARCHITECTURE.md](ARCHITECTURE.md).*

## Build, run, verify

```sh
# One-time env setup: see README.md "Setup from a clean machine".

cargo build --workspace                     # all Rust (first Tauri build ~2 min)
cargo test --workspace                      # Rust unit tests (resampler, VAD, snippets)
cd server/gateway && ./.venv/bin/python -m pytest   # gateway: rules + providers + WS e2e

# Run the stack
cd server/gateway && ./.venv/bin/uvicorn app.main:app --host 127.0.0.1 --port 8765
cargo run -p whispr-desktop                 # separate terminal, needs a display

# Cross-check Windows compatibility of the shared crates (no Windows box needed)
cargo check -p whispr-core -p whispr-insert --target x86_64-pc-windows-gnu
```

### End-to-end check without microphone or GUI

This is the fastest full-pipeline verification; use it after any change to
core, protocol, or gateway:

```sh
# terminal 1 — gateway with deterministic output
cd server/gateway
WHISPR_STT_PROVIDER=mock WHISPR_MOCK_TRANSCRIPT="um hello world this is uh a test" \
  ./.venv/bin/uvicorn app.main:app --port 8765 --log-level warning

# terminal 2 — stream a WAV (any rate/channels; it gets resampled)
python3 - <<'EOF'   # generate a 5s test wav if you don't have one
import math,struct,wave
w=wave.open("/tmp/test.wav","wb"); w.setnchannels(1); w.setsampwidth(2); w.setframerate(16000)
w.writeframes(b"".join(struct.pack("<h",int(0.3*32767*math.sin(2*3.14159*220*i/16000))) for i in range(80000)))
EOF
cargo run -q -p whispr-core --example whispr-cli -- --wav /tmp/test.wav
```

Expected: `[ready]`, growing `[partial]` lines, `[final raw]` with fillers,
`[final polished]` without them. If partials appear but no final → check the
stop/drain path in `core/src/client.rs` and the gateway's `finish()`.

### Headless GUI smoke test (CI/containers)

```sh
apt-get install -y xvfb dbus-x11
timeout 20 xvfb-run -a dbus-run-session ./target/debug/whispr-desktop
# success = no panic within the window; screenshot with x11-apps/imagemagick if needed
```

## Recipe: add an STT provider

1. Create `server/gateway/app/providers/<name>.py` with a class extending
   `SttSession` (`base.py`). Contract: `start()` (connect/load), `feed(pcm)`
   (16 kHz mono s16le bytes), `finish() -> str` (final transcript). Push
   growing partial strings to `self.partial_queue`, and push **exactly one
   `None`** when the stream ends (usually in `finish()` or your read loop).
2. Register it in `app/providers/__init__.py`: both `resolve_provider_name()`
   (auto-selection order) and `create_session()`.
3. Heavy/optional deps: import lazily inside methods and add them to
   `requirements-local.txt` (pattern: `whisper_local.py`).
4. Add a selection test in `tests/test_providers.py`; if the provider can run
   offline, add a WS test modeled on `tests/test_stream.py`.
5. Document env vars in README's provider table.

## Recipe: change the wire protocol

Touch all sync points in this order, or clients and server drift:

1. `shared/protocol.md` — the spec.
2. `core/src/protocol.rs` — serde enums (`tag = "type"`, snake_case variants).
3. `core/src/client.rs` — if the client must send/handle the new message.
4. `server/gateway/app/main.py` — parsing/emitting.
5. `server/gateway/tests/test_stream.py` — assert the new shape.
6. Run the E2E check above.

## Recipe: add a user setting

1. `core/src/config.rs` — add the field to `AppConfig` **and** its value in
   `Default` (struct has `#[serde(default)]`, so old config files still load).
2. `apps/desktop/ui/index.html` — add the input (id == serde key), wire it in
   `load()` and the save handler.
3. Consume it — usually `apps/desktop/src-tauri/src/dictation.rs::start()`
   (client-side) or pass it through `ClientMessage::Start` (server-side; that
   makes it a protocol change, see above).
4. `cargo build -p whispr-desktop && cargo test --workspace`.

## Recipe: implement a native insertion tier (AX / UIA / AT-SPI)

1. Implement the per-OS `insert()` in `platform/insert/src/accessibility.rs`
   under the existing `cfg(target_os)` gate. Return `Ok(())` only when text
   was actually inserted at the caret; any `Err` falls through to the
   clipboard-paste tier, so failing gracefully is free.
2. Add OS-specific deps in `platform/insert/Cargo.toml` under
   `[target.'cfg(target_os = "...")'.dependencies]` (e.g. `windows`, `objc2`,
   `zbus`) — keep the crate compiling on all three OSes:
   `cargo check -p whispr-insert --target x86_64-pc-windows-gnu`.
3. Detect secure/password fields and return an error naming the reason (the
   paste tier should also refuse there eventually — see R-2 in ARCHITECTURE.md).
4. Manual verification: settings window → "Test insertion (3s delay)" →
   focus a text field in another app.

## Recipe: wire VAD auto-stop

`core/src/vad.rs` is complete and tested; the gap is plumbing. In
`dictation.rs::start()`, the audio receiver is currently handed whole to
`stream_utterance`. To auto-stop: insert a relay task that owns `audio_rx`,
feeds each chunk to a `Vad` instance, forwards it to a new channel consumed by
`stream_utterance`, and fires the session's stop (`finish_session` /
`stop_tx`) on `VadEvent::SpeechEnd` when `AppConfig.auto_stop` is true.

## Recipe: tune transcript polish

- Rule-based (default): `server/gateway/app/polish.py::apply_rules` — filler
  regex `FILLER_RE` (keep it conservative; "like"/"you know" are often
  meaningful), stutter collapse, casing, punctuation. Every rule needs a case
  in `tests/test_polish.py`.
- LLM ("full" mode): `POLISH_SYSTEM` prompt + `llm_polish()` in the same file.
  Model comes from `WHISPR_POLISH_MODEL` (default `claude-haiku-4-5`). Keep
  the system block static (it carries `cache_control`), put per-request
  context (dictionary, style, app) in the user message. Must always fall back
  to `apply_rules` on any failure — never raise from `polish()`.

## Debugging playbook

| Symptom | Where to look |
|---|---|
| No partials in HUD | Settings-window event log (bottom) shows every `whispr://event`; gateway log for the WS session; `RUST_LOG=debug cargo run -p whispr-desktop` |
| Pipeline works in CLI but not desktop | Difference is capture + insertion: check mic device (`--mic` CLI flag) and insertion errors in the event log |
| Final never arrives | Provider didn't push the `None` sentinel, or `finish()` hangs — add gateway logging; client gives up after 15 s (`final_timeout` in `client.rs`) |
| Gateway 500s | `uvicorn ... --log-level debug`; every session error is logged with traceback |
| Hotkey dead on Linux | Wayland limitation (R-3) — verify via tray-menu Start/Stop instead |
| Whisper slow/failing | Check model preload line in gateway startup log; try `WHISPR_WHISPER_DEVICE=cpu WHISPR_WHISPER_COMPUTE=int8` |

## Definition of done

Before considering a change complete:

1. `cargo test --workspace` and gateway `pytest` green.
2. The no-mic E2E check passes (mock provider).
3. If protocol/config/events changed: all sync points from the recipes above
   updated (grep for the message/field name across `core/`, `server/`, `ui/`,
   `shared/`).
4. If Rust shared crates changed: Windows cross-check still passes.
5. README / docs updated if commands, env vars, or setup steps changed.
