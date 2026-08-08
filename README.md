# NovaWhisper

<div align="center">

**Speak naturally. Keep your flow.**

**English** · [Русский](README.ru.md)

</div>

## Why this project

Typing breaks your train of thought. NovaWhisper lets you dictate instead:
hold a global hotkey, speak in any application, and a polished transcript is
inserted exactly where your cursor is — no copy-pasting between a separate
dictation app and the one you're actually using.

It's built to keep dictation private and flexible rather than locking you
into one cloud vendor: speech recognition can run fully offline with a local
Whisper model, or use Deepgram when you want cloud-quality recognition. A
polish step cleans up filler words and stutters before the text lands in your
editor.

Supported today: Windows 11 (primary target, with NVIDIA CUDA acceleration),
macOS on Apple Silicon, and Linux (X11/XWayland).

## See it in action

![NovaWhisper inserting dictated text at the cursor](assets/ScreenCapture.gif)

<details>
<summary>Full walkthrough (screenshots)</summary>

| | |
|---|---|
| ![Ready status](assets/1.png) Whispr is ready | ![Hotkey and mic settings](assets/4.png) Set the global hotkey and microphone |
| ![Language and engine settings](assets/7.png) Choose language, model, and insertion method | ![Gateway ready event log](assets/6.png) Save and wait for the speech model |
| ![Start Dictation button](assets/2.png) Start dictation | ![Listening indicator](assets/3.png) Speak while Whispr listens |
| ![HUD listening](assets/9.png) Control the recording from the HUD | ![HUD transcribing](assets/10.png) Stop and let recognition run |
| ![Saved transcript](assets/5.png) Transcripts are saved to the Voice clipboard | ![Full window](assets/8.png) Manage everything from one window |
| ![Tray menu](assets/11.png) Use the tray | |

</details>

## How it works

NovaWhisper runs as two processes:

1. The Rust/Tauri **desktop app** captures the microphone, shows the HUD, and
   inserts text into the focused application.
2. The Python/FastAPI **gateway** runs speech recognition (mock / local
   Whisper / Deepgram) and polishes the transcript.

You never manage the gateway yourself: the desktop app starts it on launch
and stops it on quit, using the bundled `whispr-gateway` sidecar in packaged
builds or `server/gateway/.venv` in source builds. A gateway you started
manually is detected and left running. See
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full design.

The user flow is **configure and save → start and speak → stop and recognize
automatically → receive and use the text**, with every recognized transcript
also kept in the in-app Voice clipboard.

## Quick start (from source)

```sh
# 1. Gateway (Python)
cd server/gateway
python3 -m venv .venv   # py -m venv .venv on Windows
./.venv/bin/pip install -r requirements.txt          # mock/dev providers
./.venv/bin/pip install -r requirements-local.txt    # + local Whisper (optional)

# 2. Desktop app (Rust) — from the repository root; starts/stops the gateway itself
cargo run -p whispr-desktop
```

Then in Settings: pick a microphone, language, STT provider, and hotkey, save,
and wait for the gateway status to turn ready.

This covers the common case only. For a clean-machine Windows/CUDA walkthrough,
macOS Apple Silicon setup, Linux system packages, self-contained installers,
and troubleshooting, see **[docs/INSTALL.md](docs/INSTALL.md)**.

## Verification

```sh
cargo test --workspace
cd server/gateway && ./.venv/bin/python -m pytest
```

## Repository map

| Path | Purpose |
|---|---|
| `core/` | Rust audio, protocol, VAD, configuration, and streaming client |
| `platform/insert/` | Cross-application text insertion |
| `apps/desktop/` | Tauri desktop app and UI |
| `server/gateway/` | FastAPI gateway, STT providers, and transcript polish |
| `scripts/` | `build-gateway.sh` / `.ps1` — self-contained gateway sidecar builds |
| `shared/protocol.md` | Client/gateway wire protocol |
| `docs/` | Installation, architecture, structure, and development recipes |

## Documentation

- [docs/INSTALL.md](docs/INSTALL.md) — full platform setup, packaging, troubleshooting
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — stack rationale, roadmap, risk register
- [docs/STRUCTURE.md](docs/STRUCTURE.md) — file map, runtime picture, invariants
- [docs/SKILLS.md](docs/SKILLS.md) — task recipes for common changes
