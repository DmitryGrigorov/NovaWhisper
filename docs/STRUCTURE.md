# Project Structure

*Map of the codebase for humans and agents. One line per file: what it is and
when to touch it. See [SKILLS.md](SKILLS.md) for task recipes and
[ARCHITECTURE.md](ARCHITECTURE.md) for the why.*

## Runtime picture

```
Desktop app (Rust/Tauri)                Gateway (Python/FastAPI)
─────────────────────────               ────────────────────────
hotkey ──► dictation.rs                 /v1/stream (WebSocket)
             │ AudioCapture (cpal)        │ SttSession (mock | whisper_local | deepgram)
             │ 16kHz PCM chunks           │   partial_queue ──► "partial" frames
             ▼                            ▼
           stream_utterance ──WS──►     receive audio ──► STT ──► polish()
             │ StreamEvents ◄──WS──     "final" {text, raw_text}
             ▼
           snippets.expand ──► whispr_insert::insert_text ──► focused app
           HUD events (whispr://event) ──► ui/hud.html + ui/index.html
```

## File map

### `core/` — whispr-core (Rust lib, shared by all clients)

| File | What it is | Touch it when |
|---|---|---|
| `src/protocol.rs` | Serde types for the wire protocol (`ClientMessage`, `ServerMessage`; `tag="type"`, snake_case) | Protocol changes — **sync point**, see Invariants |
| `src/audio.rs` | `AudioCapture` (cpal → dedicated thread → 16 kHz mono i16 chunks via tokio mpsc), `LinearResampler`, `LevelProbe` (mic meter), `resample_to_target()` | Capture bugs, resampling, new audio formats (Opus) |
| `src/client.rs` | `stream_utterance()` — one WS connection per utterance; forwards audio, emits `StreamEvent`s; stop via oneshot (dropped sender = "end on audio close") | Protocol changes, reconnect logic, timeouts |
| `src/vad.rs` | Energy VAD with hysteresis/hangover (`Vad::push` → `SpeechStart/End`). **Built + tested but not yet wired into the desktop app** | Implementing auto-stop |
| `src/dictionary.rs` | `SnippetEngine::expand()` — whole-utterance and inline multi-word trigger replacement | Snippet matching rules |
| `src/config.rs` | `AppConfig` (serde, `#[serde(default)]`) persisted at `config_dir()/whispr/config.json` | Adding a setting — **sync point** with settings UI |
| `examples/whispr_cli.rs` | CLI harness: stream a WAV or mic to the gateway, print partials/final. Primary no-GUI E2E tool | Testing pipeline changes |

### `platform/insert/` — whispr-insert (Rust lib)

| File | What it is | Touch it when |
|---|---|---|
| `src/lib.rs` | `insert_text(text, InsertMethod)` — tiered: accessibility → clipboard-paste (arboard + enigo Ctrl/Cmd+V, saves & restores clipboard) → keystroke typing | Insertion behavior, fallback order |
| `src/accessibility.rs` | Per-OS tier-1 stubs (`cfg(target_os)`); all currently return `Err` → falls through to paste | Implementing native AX (macOS) / UIA (Windows) / AT-SPI (Linux) |

### `apps/desktop/` — whispr-desktop (Tauri 2)

| File | What it is | Touch it when |
|---|---|---|
| `src-tauri/src/main.rs` | Tauri builder: tray menu, global-shortcut plugin + registration, window creation (settings `main`, HUD `hud`), commands `get_config`/`save_config`/`toggle_dictation`/`test_insert` | New commands, windows, tray items |
| `src-tauri/src/dictation.rs` | Session state machine: `toggle()` start/stop, capture → stream → events → snippet expand → insert; HUD show/hide/position; emits `whispr://event` | Dictation flow, event payloads — **sync point** with UI |
| `src-tauri/tauri.conf.json` | App config: `frontendDist: "../ui"`, `withGlobalTauri: true`, bundle icons | Windows/bundle/config changes |
| `src-tauri/capabilities/default.json` | IPC permissions for windows `main` + `hud` (`core:default`) | Adding a window (add its label here) or JS plugin APIs |
| `src-tauri/icons/` | Generated PNGs (script: scratchpad `make_icons.py`, checked-in output) | Rebranding |
| `ui/index.html` | Settings page (vanilla JS, `window.__TAURI__`): loads/saves `AppConfig`, event log | New settings — field ids mirror `AppConfig` keys |
| `ui/hud.html` | Recording overlay: live partial text, pulsing dot, mic level bar; listens to `whispr://event` | HUD look/behavior |

### `server/gateway/` — Python FastAPI

| File | What it is | Touch it when |
|---|---|---|
| `app/main.py` | WS `/v1/stream` endpoint (start → audio frames → stop → final), `/healthz`, lifespan preloads local Whisper | Protocol changes — **sync point** |
| `app/providers/base.py` | `SttSession` ABC: `start()/feed()/finish()` + `partial_queue` (`None` = end sentinel) | Provider interface changes |
| `app/providers/mock.py` | Offline provider: reveals canned transcript ~2.5 words/sec of audio (`WHISPR_MOCK_TRANSCRIPT`) | Test/dev behavior |
| `app/providers/whisper_local.py` | faster-whisper on local GPU/CPU; 1 s incremental partial decodes (beam 1), final beam 5; module-level model cache | Local STT tuning |
| `app/providers/deepgram.py` | Deepgram streaming WS client (untested against live API — no key in dev) | Cloud STT |
| `app/providers/__init__.py` | `resolve_provider_name()` (env override → deepgram-if-key → whisper-if-installed → mock) + `create_session()` | Registering a provider |
| `app/polish.py` | `apply_rules()` (fillers, stutters, casing, punctuation, dictionary casing) and `llm_polish()` (Claude `claude-haiku-4-5`, prompt-cached system block, falls back to rules on any failure) | Polish behavior, prompts |
| `tests/` | `test_polish.py` (rules), `test_providers.py` (selection), `test_stream.py` (WS end-to-end with mock) | Any gateway change — keep green |
| `requirements.txt` / `requirements-local.txt` | Base deps / optional faster-whisper | Dependency changes |

### Other

| Path | What it is |
|---|---|
| `shared/protocol.md` | **Normative** wire-protocol spec — update first, then the two implementations |
| `docs/ARCHITECTURE.md` | Founding plan: stack decisions, P0–P2 roadmap, risk register (R-1…R-8) |
| `docs/SKILLS.md` | Task recipes for common changes |
| `Cargo.toml` (root) | Workspace: `core`, `platform/insert`, `apps/desktop/src-tauri` |

## Invariants & sync points

1. **Wire protocol lives in three places** and must stay in sync:
   `shared/protocol.md` (spec) ⇄ `core/src/protocol.rs` ⇄
   `server/gateway/app/main.py` (+ `tests/test_stream.py`). Audio is raw PCM
   s16le, 16 kHz, mono, binary WS frames; control messages are JSON text
   frames tagged by `"type"`.
2. **Desktop event contract**: Rust emits Tauri event `whispr://event` with
   `kind` ∈ `status{recording} | ready | partial{text} | final{text,raw_text,duration_ms} | inserted{via} | error{message} | level{value}`.
   Consumers: `ui/hud.html`, `ui/index.html`. Change in `dictation.rs` → update both pages.
3. **Config field names**: `AppConfig` serde keys == element ids in
   `ui/index.html` (`gateway_url`, `hotkey`, `language`, `polish_mode`,
   `insert_method`, `dictionary`, `snippets`). New fields need
   `#[serde(default)]`-compatible defaults so old config files keep loading.
4. **Provider contract**: exactly one `None` pushed to `partial_queue` when
   the session ends (the gateway's forwarder task blocks on it).
5. **cpal `Stream` is `!Send`** — it must stay on the dedicated capture
   thread (`audio.rs`); never move it into async code.
6. **New Tauri windows** must be added to `capabilities/default.json`
   `windows` list or their JS gets no IPC.

## Known gaps (deliberate, on the roadmap)

- `accessibility.rs` tiers are stubs → paste tier does all insertion.
- VAD exists but auto-stop isn't wired (`auto_stop` config flag is read but unused).
- Audio is uncompressed PCM (Opus planned).
- Deepgram provider and `llm_polish` are implemented but not exercised against
  live APIs in CI.
- Linux Wayland: global hotkey and enigo-paste need X11/XWayland (see R-3 in
  ARCHITECTURE.md).
- No mobile apps, no sync service yet (P1/P2).
