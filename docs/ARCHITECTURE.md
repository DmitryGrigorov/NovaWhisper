# Whispr — Architecture & Implementation Plan

*A cross-platform AI voice-to-text dictation tool (Wispr Flow alternative).*

> **Doc map:** this file is the founding plan — stack decisions, roadmap,
> risk register. For the code as it exists, read
> [STRUCTURE.md](STRUCTURE.md) (file map, invariants, sync points) and
> [SKILLS.md](SKILLS.md) (build/run/test commands and task recipes).
> Wire protocol: [../shared/protocol.md](../shared/protocol.md).

## 0. As-built status (2026-07-10)

The P0 vertical slice is implemented and verified end-to-end on desktop:
capture (cpal) → 16 kHz PCM over WebSocket → STT → polish → cross-app
insertion, with tray + global hotkey + live HUD. Deviations from the plan
below, all deliberate:

- **One insertion crate** (`platform/insert/`) with tiered fallback instead of
  three per-OS crates; the native AX/UIA/AT-SPI tiers are stubs and the
  clipboard-paste tier does the work (per R-2, paste is the workhorse anyway).
- **Local Whisper STT** (`whisper_local`, faster-whisper on CUDA) was pulled
  forward from the P2 privacy-mode plan because the primary target machine is
  a Windows 11 PC with an RTX 4090. A zero-config **mock provider** was added
  for offline development and CI.
- **VAD is built and tested but not yet wired** to auto-stop (hotkey toggle is
  the P0 interaction). Audio is raw PCM; Opus framing deferred.
- Mobile apps, sync service, and the iOS mic spike (R-1) have not started.

---

## 1. Repository Analysis

The repository (`/root/works/whispr`) contained no code, dependencies, or configuration at
the start of this project. There is nothing to reuse, which means the stack can be chosen
purely on technical merit. This document is the founding plan.

## 2. Stack Decision

### Desktop: **Tauri 2 + Rust** (Option A)

Chosen over Electron and Flutter Desktop because every hard problem in this product is a
native-systems problem, and Rust has the best story for all of them:

| Concern | Rust/Tauri answer |
|---|---|
| Low-latency audio capture | `cpal` (CoreAudio / WASAPI / PipeWire-ALSA backends) |
| Global hotkeys | `tauri-plugin-global-shortcut`, `global-hotkey` crate |
| System tray / menu bar | Built into Tauri 2 |
| Text insertion | FFI per OS: `objc2` + AX APIs (macOS), `windows-rs` + UIAutomation (Windows), `zbus`/`atspi` (Linux) |
| Keystroke/clipboard fallback | `enigo`, `arboard` |
| Always-on background footprint | ~30–60 MB RSS vs. Electron's ~150–300 MB — matters for a resident utility |
| Code sharing with mobile | The Rust core compiles for Android/iOS via **UniFFI** |

Electron would need native Node addons for all of the above anyway (losing its "fastest
development" advantage), and Flutter Desktop's system-level access (accessibility APIs,
event taps) is the weakest of the three.

The Tauri webview (React + TypeScript) is used only for the settings/dashboard UI and the
recording HUD overlay — none of the latency-critical path touches JS.

### Mobile: **Native shells (Kotlin / Swift) + shared Rust core** (Option C, augmented)

Flutter/KMM share the *easy* code. On mobile, ~80% of the work is inherently
platform-specific (Android IME & AccessibilityService, iOS keyboard extension & App
Groups), so cross-platform UI frameworks buy little. Instead:

- **Shared Rust `core` crate** (audio framing, VAD, streaming protocol client, dictionary
  matcher, snippet engine, settings model) compiled via UniFFI → Kotlin & Swift bindings.
- **Android**: Kotlin + Jetpack Compose. Primary surface is a **custom IME
  (InputMethodService)** with a mic key — this is what Wispr Flow itself ships, it inserts
  text via `InputConnection` (no accessibility permission needed), and it avoids Google
  Play's AccessibilityService policy wall. The floating bubble
  (`SYSTEM_ALERT_WINDOW`) + AccessibilityService insertion is a secondary, sideload-friendly mode.
- **iOS**: Swift + SwiftUI. Custom keyboard extension with `RequestsOpenAccess`, container
  app for onboarding/settings, App Group for shared state. **See risk R-1 below — this is
  the highest-risk item in the whole project and must be spiked in week 1.**

### Backend: **Python + FastAPI** (with a Go escape hatch)

- FastAPI handles WebSocket audio streaming comfortably at MVP/early-growth scale and has
  the richest AI-provider ecosystem (all STT/LLM SDKs are Python-first).
- The audio gateway is isolated as its own service from day one so it can be rewritten in
  Go later if concurrent-connection counts demand it, without touching the rest.
- **Supabase** for auth, Postgres, and cross-device sync of dictionary/snippets/settings
  (row-level security, realtime subscriptions, self-hostable; HIPAA available on paid tiers).

### AI Providers: switchable adapters, two of each

- **STT** (`SttProvider` interface): **Deepgram** (best streaming latency) and **Azure
  Speech** (~100+ locales, strong auto-detect) as the two launch providers; **whisper.cpp**
  (local, Metal/CUDA) as the offline/privacy provider. Mixed-language input favors
  Whisper-family models.
- **LLM polish** (`PolishProvider` interface): **Claude Haiku 4.5** (`claude-haiku-4-5-20251001`)
  as default — fast + cheap enough for per-utterance calls, with prompt caching for the
  static system prompt + user dictionary; GPT-4o-mini as the alternate. A local Llama 3.x
  path is deferred to P2 privacy mode.

## 3. System Architecture

```
┌────────────── Client (any platform) ──────────────┐
│  Hotkey/mic-key trigger                            │
│  cpal / AudioRecord / AVAudioEngine capture        │
│  Rust core: VAD → 100–200 ms Opus frames           │
│  ── WebSocket (wss, TLS 1.3) ──────────────────────┼──► ┌── Gateway (FastAPI /stream) ──┐
│  ◄─ partial transcripts (live HUD display)         │    │ session auth (JWT)            │
│  ◄─ final polished text                            │    │ ► SttProvider (streaming)     │
│  Insertion engine: AX / UIA / AT-SPI /             │    │ ► PolishProvider (on final)   │
│    InputConnection / keyboard-ext insertText,      │    │   + dictionary & snippets     │
│    with clipboard-paste + synthetic-key fallbacks  │    │   + app-context style hints   │
└────────────────────────────────────────────────────┘    └──────────┬────────────────────┘
                                                                     │
                                              ┌── Sync API (FastAPI + Supabase) ──┐
                                              │ auth, dictionary, snippets,        │
                                              │ settings, usage; realtime push     │
                                              └────────────────────────────────────┘
```

**Latency budget** (stop-speaking → text inserted): ≤ 1.5 s.
Streaming STT emits partials during speech (~300 ms behind live); on end-of-speech the
final transcript goes through one fast LLM call (~400–700 ms with Haiku + prompt caching);
insertion is ~10–50 ms. Filler-word-only polish (P0) can be regex/rule-based at ~0 ms.

**Insertion strategy (all desktop platforms)** — three-tier fallback, in order:
1. Accessibility API insertion at caret (AX / UIA ValuePattern / AT-SPI EditableText).
2. Clipboard save → set text → synthetic paste (Cmd/Ctrl+V) → restore clipboard.
3. Synthetic keystrokes (slowest, always works except secure fields).
Tier 1 fails in a *lot* of real apps (Electron apps, terminals, games); tier 2 is the
workhorse in practice and must be first-class, not an afterthought.

## 4. Monorepo Layout

*(planned; as built, the three `platform/insert-*` crates are one
`platform/insert/` crate with per-OS `cfg` modules — see STRUCTURE.md)*

```
whispr/
├── core/                    # Rust: audio pipeline, VAD, Opus, WS client, dictionary,
│                            #   snippets, settings model; UniFFI bindings
├── platform/
│   ├── insert-macos/        # AXUIElement insertion + CGEventTap helpers
│   ├── insert-windows/      # IUIAutomation + SendInput fallback
│   └── insert-linux/        # AT-SPI (zbus) + wtype/XTEST fallback
├── apps/
│   ├── desktop/             # Tauri 2 (macOS/Win/Linux): tray, hotkeys, HUD, settings UI
│   ├── android/             # Kotlin/Compose: IME, overlay mode, foreground service
│   └── ios/                 # Swift/SwiftUI: keyboard extension + container app
├── server/
│   ├── gateway/             # FastAPI WS audio gateway, STT/LLM adapters
│   └── sync/                # FastAPI + Supabase: auth, dictionary, snippets, settings
├── shared/                  # protocol schema (JSON Schema/protobuf), prompt templates
└── docs/
```

## 5. Roadmap & Effort Estimates

Estimates assume one experienced developer per line; phases within a platform are
sequential, platforms are parallelizable. "d" = working days.

### P0 — MVP (~4–5 months solo; ~2 months with 3 devs)

| # | Work item | Est. | Notes |
|---|---|---|---|
| 0.1 | Monorepo scaffold, CI, protocol schema | 3 d | |
| 0.2 | **iOS keyboard-mic spike (risk R-1)** | 2 d | Go/no-go gate for iOS approach |
| 0.3 | Rust core: cpal capture, VAD, Opus framing, WS client | 12 d | |
| 0.4 | Gateway: WS session, Deepgram adapter, rule-based filler removal | 8 d | |
| 0.5 | Desktop shell: tray, global hotkey, HUD overlay, settings UI | 12 d | |
| 0.6 | macOS insertion (AX + paste fallback) + TCC permission flow | 6 d | |
| 0.7 | Windows insertion (UIA + SendInput/paste fallback) | 6 d | |
| 0.8 | Linux insertion (AT-SPI + wtype fallback) + portal hotkeys | 9 d | Wayland pain, see R-3 |
| 0.9 | Android: IME with mic key, AudioRecord, InputConnection insert | 16 d | |
| 0.10 | iOS: keyboard ext + container app + App Group handoff | 16 d | High variance (R-1) |
| 0.11 | Packaging: DMG + notarization, MSI + signing, .deb/AppImage | 6 d | |

### P1 — Core Experience (~6–8 weeks)

| # | Work item | Est. |
|---|---|---|
| 1.1 | LLM polish pipeline (grammar, punctuation, paragraphs) + prompt caching | 8 d |
| 1.2 | Personal dictionary: local store, Supabase sync, STT keyword-boost + LLM bias | 8 d |
| 1.3 | 100+ language support: provider routing, auto-detect, mixed-language handling | 8 d |
| 1.4 | Snippets: voice-trigger matcher (Rust core) + sync + management UI | 6 d |
| 1.5 | Accounts/auth across all clients | 5 d |

### P2 — Differentiators (~8–12 weeks)

| # | Work item | Est. |
|---|---|---|
| 2.1 | Frontmost-app detection per OS (NSWorkspace / Win32 / portal+X11, IME package name) | 6 d |
| 2.2 | Style adaptation: per-app tone profiles feeding the polish prompt | 8 d |
| 2.3 | Context-aware formatting (email vs. IDE vs. chat) | 6 d |
| 2.4 | Zero-data-retention mode: no server persistence, provider ZDR, local whisper.cpp | 10 d |
| 2.5 | Enterprise packaging: MDM profiles, silent MSI, org policies | 8 d |
| 2.6 | SOC 2 / HIPAA groundwork: audit logging, encryption at rest, BAA-ready providers | 10 d+ |

## 6. Risk Register (pitfalls to design around)

- **R-1 · iOS keyboard microphone (CRITICAL, spike first).** Keyboard extensions
  historically cannot access the microphone, and even with Full Access they run under a
  ~60 MB memory cap. Modern voice keyboards do ship on iOS 18, but the exact mechanism
  (in-extension recording vs. container-app handoff via App Groups/deep link) must be
  proven on-device before committing to the iOS architecture. Budget the 2-day spike
  before any other iOS work.
- **R-2 · Desktop accessibility insertion is unreliable.** AX/UIA/AT-SPI fail in Electron
  apps, terminals, canvas-rendered editors, and secure input fields. The clipboard-paste
  fallback is the real workhorse; build and test it first, and detect secure fields
  (macOS `kAXSecureTextField`, Windows password controls) to fail gracefully.
- **R-3 · Ubuntu 26 = GNOME Wayland.** X11-style global hotkey grabs don't exist on
  Wayland — use the XDG Desktop Portal `GlobalShortcuts` interface (compositor-dependent
  UX) with an X11 fallback. AT-SPI insertion coverage is spotty (GTK apps OK; Electron/Qt
  inconsistent); fall back to `wtype` (virtual-keyboard protocol) or `ydotool` (uinput,
  needs permission setup). Audio via PipeWire is the easy part.
- **R-4 · Google Play AccessibilityService policy.** Play routinely rejects
  non-accessibility uses. The IME is the primary Android surface; ship the
  overlay+AccessibilityService mode as opt-in/sideload. Android 14+ also requires
  `foregroundServiceType="microphone"` and runtime justification for background capture.
- **R-5 · macOS permissions & distribution.** TCC prompts for Microphone + Accessibility
  (and Input Monitoring if using CGEventTap); permissions reset on binary signature change
  during development. Hardened runtime + notarization required for distribution; build the
  onboarding flow that walks users through System Settings toggles.
- **R-6 · Windows signing & hooks.** Unsigned installers hit SmartScreen walls (EV cert or
  reputation build-up). Keep the `WH_KEYBOARD_LL` hook callback trivial (post to a queue)
  — a slow hook adds latency to *all* system input and Windows silently removes it.
- **R-7 · Streaming STT language coverage.** Deepgram's streaming languages < Whisper's
  ~100. The "100+ languages" claim requires Azure or Whisper-family routing; mixed-language
  utterances need Whisper-class models. Keep the provider abstraction honest (feature
  flags per provider: streaming?, auto-detect?, code-switching?).
- **R-8 · Compliance is procedural, not just technical.** SOC 2 Type II needs an audit
  window; HIPAA needs BAAs with every subprocessor (STT/LLM providers, hosting). Choose
  providers with ZDR/BAA options now (Azure, Anthropic, OpenAI enterprise) so P2 doesn't
  force a migration.

## 7. Build Order (first 6 weeks, single dev)

1. **Week 1**: Monorepo scaffold; iOS mic spike (R-1); protocol schema; `core` crate with
   cpal capture → WAV file proof.
2. **Week 2**: Gateway WS + Deepgram streaming; `core` WS client; end-to-end partial
   transcripts printing in a CLI test harness.
3. **Week 3**: Tauri shell — tray, hotkey, HUD; wire to core; rule-based filler removal.
4. **Week 4**: macOS insertion tiers 1–2 + TCC onboarding → **first usable dogfood build**.
5. **Week 5**: Windows insertion + packaging; start Linux portal/AT-SPI work.
6. **Week 6**: Linux insertion + fallbacks; dogfood across all three desktops.

Mobile starts after desktop dogfood (or in parallel with a second dev), reusing the
by-then-hardened core crate and gateway.
