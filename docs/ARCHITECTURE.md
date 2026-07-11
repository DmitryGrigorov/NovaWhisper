# NovaWhisper — architecture

This document summarizes the architecture as implemented in the current repository. For the detailed file map and change recipes, use [STRUCTURE.md](STRUCTURE.md) and [SKILLS.md](SKILLS.md). The wire protocol spec lives in [../shared/protocol.md](../shared/protocol.md).

## System overview

NovaWhisper is a desktop dictation tool with two cooperating processes:

1. A Rust/Tauri desktop app handles audio capture, the HUD, settings, hotkeys, and text insertion.
2. A Python/FastAPI gateway performs speech recognition and transcript polishing.

The runtime flow is:

- capture microphone audio with `cpal`
- resample to 16 kHz mono PCM
- stream audio over WebSocket to the gateway
- emit partial and final transcript events back to the desktop app
- expand snippets and insert the final text into the focused application

## Main components

### Desktop app

The desktop application lives in [apps/desktop](../apps/desktop) and is built around Tauri 2.

Key responsibilities:

- manage the global hotkey and tray/menu experience
- show the live HUD and settings UI
- coordinate recording sessions and event emission
- start and stop the local gateway when configured to do so
- route insertion through the shared insertion crate

The main implementation points are:

- [apps/desktop/src-tauri/src/dictation.rs](../apps/desktop/src-tauri/src/dictation.rs)
- [apps/desktop/src-tauri/src/gateway.rs](../apps/desktop/src-tauri/src/gateway.rs)
- [apps/desktop/ui/index.html](../apps/desktop/ui/index.html)
- [apps/desktop/ui/hud.html](../apps/desktop/ui/hud.html)

### Shared Rust core

The reusable core library is in [core](../core) and contains the logic that is shared across clients and the desktop app.

It includes:

- audio capture and resampling in [core/src/audio.rs](../core/src/audio.rs)
- protocol models in [core/src/protocol.rs](../core/src/protocol.rs)
- streaming client logic in [core/src/client.rs](../core/src/client.rs)
- VAD support in [core/src/vad.rs](../core/src/vad.rs)
- settings and config in [core/src/config.rs](../core/src/config.rs)
- snippet expansion in [core/src/dictionary.rs](../core/src/dictionary.rs)

### Gateway

The Python gateway is in [server/gateway](../server/gateway) and exposes a FastAPI WebSocket endpoint for audio streaming.

It is responsible for:

- accepting audio from the desktop client
- running an STT provider session
- pushing partial transcripts and a final transcript
- applying polish rules or LLM-based polishing
- returning the final text to the client

The main implementation points are:

- [server/gateway/app/main.py](../server/gateway/app/main.py)
- [server/gateway/app/polish.py](../server/gateway/app/polish.py)
- [server/gateway/app/providers](../server/gateway/app/providers)

### Insertion layer

Text insertion is implemented in [platform/insert](../platform/insert).

The current design is a tiered fallback:

1. native accessibility insertion where available
2. clipboard-paste fallback
3. synthetic keystroke fallback

The accessibility tiers are present as stubs today, so the paste-based path is the practical workhorse.

## Current implementation status

The desktop path is implemented end to end and verified in the repository workflow:

- microphone capture and audio framing
- WebSocket streaming to the gateway
- mock, local Whisper, and Deepgram provider support
- rule-based and LLM-based transcript polishing
- HUD updates and settings-driven config
- managed gateway startup and shutdown for source builds and packaged builds

Items still on the roadmap or partially wired:

- VAD-driven auto-stop is implemented in the core crate but not fully wired into the desktop session flow
- native accessibility insertion tiers are still stubbed on most platforms
- mobile targets and sync services are not part of the current repository scope

## Design invariants

Several implementation rules are important for keeping the system coherent:

- the wire protocol must stay synchronized across [shared/protocol.md](../shared/protocol.md), [core/src/protocol.rs](../core/src/protocol.rs), [server/gateway/app/main.py](../server/gateway/app/main.py), and [server/gateway/tests/test_stream.py](../server/gateway/tests/test_stream.py)
- every STT provider must push exactly one `None` sentinel to the partial queue when a session ends
- `polish()` must never raise; the LLM path must fall back to rule-based polish on failure
- the `cpal` stream must remain on the dedicated capture thread
- new config fields need serde defaults and matching UI ids in [apps/desktop/ui/index.html](../apps/desktop/ui/index.html)
- gateway lifecycle is owned by [apps/desktop/src-tauri/src/gateway.rs](../apps/desktop/src-tauri/src/gateway.rs), and externally started gateways are reused rather than killed

## Near-term priorities

The current work focuses on reliability and polish rather than a new architecture:

- finish VAD-driven auto-stop
- harden insertion fallback behavior and platform-specific native insertion
- keep packaging and gateway discovery robust across source and bundled builds
- maintain the no-mic end-to-end check and the Rust/Python test suites as the release gate
