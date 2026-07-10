# Whispr

Cross-platform AI voice-to-text dictation — press a global hotkey in any app,
speak, and polished text is inserted where your cursor is. A Wispr Flow
alternative. Architecture, roadmap, and platform strategy live in
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

**How it works:** the desktop app captures your microphone and streams audio
over a local WebSocket to a small gateway server, which runs speech-to-text
(locally on your GPU, or via Deepgram) and cleans up the transcript (filler
removal, or a full LLM polish). The final text is typed into whatever app you
were using. Two processes: the **gateway** and the **desktop app**.

| Path | What it is |
|---|---|
| `core/` | Rust: audio capture, VAD, streaming client, snippets, config |
| `platform/insert/` | Rust: cross-app text insertion (clipboard-paste + typing tiers) |
| `apps/desktop/` | Tauri 2 app: tray, global hotkey, recording HUD, settings UI |
| `server/gateway/` | FastAPI WebSocket gateway: STT providers + transcript polish |
| `shared/protocol.md` | Client ↔ gateway wire protocol |

---

## Setup from a clean machine

### Step 1 — Install prerequisites

**All platforms** need three things: Git, Python 3.10+, and Rust.

```sh
# Rust (any platform) — installs cargo + the toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

**Ubuntu / Debian Linux** — additionally install the system libraries for
Tauri (webview, tray), audio, and input synthesis:

```sh
sudo apt-get update
sudo apt-get install -y \
  git python3-venv build-essential pkg-config libssl-dev \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev libasound2-dev libxdo-dev patchelf
```

**Windows 11:**

1. [Rust via rustup](https://rustup.rs) — pick the default **MSVC** toolchain.
2. [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
   with the **"Desktop development with C++"** workload (the Rust installer
   offers to do this for you).
3. [Python 3.11+](https://www.python.org/downloads/) — check *"Add python.exe
   to PATH"* in the installer.
4. [Git for Windows](https://git-scm.com/download/win).
5. WebView2 ships with Windows 11 — nothing to install for the UI.

**macOS** *(builds, but untested — the native insertion tier is not written
yet)*:

```sh
xcode-select --install     # compilers + git
brew install python        # if you don't have Python 3.10+
```

### Step 2 — Clone and start the gateway

```sh
git clone <your-remote>/whispr.git && cd whispr

# Linux/macOS
cd server/gateway
python3 -m venv .venv
./.venv/bin/pip install -r requirements.txt
./.venv/bin/uvicorn app.main:app --host 127.0.0.1 --port 8765
```

```powershell
# Windows (PowerShell)
cd server\gateway
py -m venv .venv
.venv\Scripts\pip install -r requirements.txt
.venv\Scripts\uvicorn app.main:app --host 127.0.0.1 --port 8765
```

Leave it running. Check it's alive: `curl http://127.0.0.1:8765/healthz` →
`{"status":"ok"}`.

**Which speech-to-text runs?** The gateway picks automatically:

| Provider | When it's used | Setup |
|---|---|---|
| **mock** | Default on a clean machine | None. Emits a canned transcript so you can verify the whole pipeline before configuring STT. |
| **whisper_local** | `faster-whisper` installed | `pip install -r requirements-local.txt`. Fully offline; GPU-accelerated (see Step 4). |
| **deepgram** | `DEEPGRAM_API_KEY` set | Cloud streaming STT, ~100 ms partials. |

Force one with `WHISPR_STT_PROVIDER=mock|whisper_local|deepgram`.
For LLM transcript polish ("full" mode in settings), also set
`ANTHROPIC_API_KEY`; without it, polish falls back to fast rule-based cleanup.

### Step 3 — Build and run the desktop app

From the repo root, in a second terminal:

```sh
cargo run -p whispr-desktop
```

The first build takes a few minutes (Tauri + webview bindings); after that
it's seconds. You get:

- a **tray icon** (menu: Start/Stop Dictation, Settings, Quit),
- the **settings window** — gateway URL, hotkey, language, polish mode,
  insertion method, personal dictionary, snippets. Config persists to
  `~/.config/whispr/config.json` (Linux), `%APPDATA%\whispr\` (Windows),
  `~/Library/Application Support/whispr/` (macOS),
- a hidden **HUD** that appears while dictating with live transcript + mic
  level.

### Step 4 — Dictate

1. Focus any text field in any app.
2. Press **Ctrl+Shift+Space** (change it in Settings).
3. Speak. The HUD shows live partial transcripts.
4. Press the hotkey again. The polished text is inserted at your cursor.

On the mock provider you'll see the canned sentence — that proves capture →
streaming → polish → insertion works. Then switch to a real STT provider:

**Local GPU (recommended if you have an NVIDIA card, e.g. RTX 4090):**

```powershell
# in server/gateway, with the venv active
pip install -r requirements-local.txt

$env:WHISPR_STT_PROVIDER = "whisper_local"
$env:WHISPR_WHISPER_DEVICE = "cuda"           # "cpu" works too, slower
$env:WHISPR_WHISPER_COMPUTE = "float16"       # "int8" for CPU
$env:WHISPR_WHISPER_MODEL = "large-v3-turbo"  # ~1.6 GB download on first run
uvicorn app.main:app --host 127.0.0.1 --port 8765
```

CUDA needs the **CUDA 12.x + cuDNN 9** runtimes from NVIDIA. Not installed?
Use `WHISPR_WHISPER_DEVICE=cpu` + `WHISPR_WHISPER_COMPUTE=int8` — slower but
works everywhere, and audio still never leaves your machine. The model is
preloaded at gateway startup, so the first dictation is as fast as the rest.

**Cloud (Deepgram):** `export DEEPGRAM_API_KEY=...` and restart the gateway.

---

## Verifying without a microphone or GUI

The CLI harness streams a WAV file through the exact same pipeline:

```sh
cargo run -p whispr-core --example whispr-cli -- --wav path/to/audio.wav
cargo run -p whispr-core --example whispr-cli -- --mic --seconds 5
```

It prints `[partial]` lines as they stream and the raw + polished final.

## Tests

```sh
cargo test --workspace                              # resampler, VAD, snippets
cd server/gateway && ./.venv/bin/python -m pytest   # polish, providers, WS end-to-end
```

## Troubleshooting

| Symptom | Fix |
|---|---|
| `cargo build` fails mentioning `webkit2gtk`, `gtk`, `appindicator`, `alsa`, or `xdo` | Install the Ubuntu package list from Step 1 (names differ slightly on Fedora/Arch). |
| Windows build fails with `link.exe not found` | Install VS Build Tools with the C++ workload, then restart the terminal. |
| HUD shows `⚠ failed to connect to gateway` | Gateway isn't running, or the Gateway URL in Settings doesn't match (default `ws://127.0.0.1:8765/v1/stream`). |
| Dictation always types the same canned sentence | You're on the **mock** provider — install faster-whisper or set a Deepgram key (Step 4). |
| Whisper fails with a CUDA/cuDNN error | Install CUDA 12.x + cuDNN 9, or set `WHISPR_WHISPER_DEVICE=cpu` and `WHISPR_WHISPER_COMPUTE=int8`. |
| First local-whisper start is slow | One-time model download (~1.6 GB for `large-v3-turbo`); watch the gateway log. |
| Hotkey does nothing on Linux | On GNOME **Wayland**, global X11-style hotkey grabs don't work yet — use the tray menu's Start/Stop, or log into an Xorg session. (Portal-based shortcuts are on the roadmap.) |
| Text isn't inserted on Linux | Insertion currently needs X11/XWayland (clipboard-paste tier). Wayland-native insertion (`wtype`/AT-SPI) is on the roadmap. |
| `microphone unavailable` error | No default input device, or another app holds it exclusively. Test with the CLI harness `--mic`. |
| Hotkey rejected at startup | Invalid combo in config — format is like `ctrl+shift+space`, `alt+d`. Fix it in Settings and save. |

## Status

P0 vertical slice: capture → stream → STT (mock/local Whisper/Deepgram) →
polish (rules/Claude) → insert, working end-to-end on desktop. Not yet built:
native accessibility insertion tiers (clipboard-paste is used), Opus framing,
VAD auto-stop, mobile apps, sync service. Roadmap and risk register:
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
