# Voice Desktop Agent

A Windows 11 desktop assistant you talk to like a phone call. It opens apps, reads the screen, and helps with everyday tasks, and it always asks before anything important.

> **Status: Phase 5.** Talk or type in your own words. Azure Speech listens, Google Gemini understands, Windows voices answer. The only computer action so far is opening your default browser, and the app checks that it worked. It **cannot yet hear, speak, or control your computer**, and the UI says so. See [CHECKLIST.md](CHECKLIST.md).

## Prerequisites (Windows 11 x64)

| Tool | Verified version | Install |
|---|---|---|
| Node.js | 24.x (npm 11) | https://nodejs.org |
| Rust (MSVC toolchain) | 1.99 stable | https://rustup.rs |
| Visual Studio Build Tools | 2026, "Desktop development with C++" | https://visualstudio.microsoft.com/downloads/ |
| Windows SDK | 10.0.26100 | included with the Build Tools workload |
| WebView2 Runtime | preinstalled on Windows 11 | https://developer.microsoft.com/microsoft-edge/webview2/ |

## Run

```powershell
npm install
npm run tauri dev        # launches the desktop app (first Rust build takes a few minutes)
```

## Test

```powershell
npm test                 # frontend unit tests (state machine, IPC schemas)
npm run test:rust        # Rust unit tests
npm run build            # type-check + production frontend build
# Real desktop smoke tests via UI Automation (after `npm run tauri build`):
Start-Process src-tauri	arget
eleaseoice-desktop-agent.exe
powershell -ExecutionPolicy Bypass -File scripts/smoke-test.ps1          # UI, IPC, typed request (no side effects)
# VISIBLE SIDE EFFECT, opens your default browser:
powershell -ExecutionPolicy Bypass -File scripts/smoke-open-browser.ps1
# Sends a deliberately FAKE key to Microsoft and checks it is rejected and not stored:
powershell -ExecutionPolicy Bypass -File scripts/smoke-speech-setup.ps1
# close the app, then:
powershell -ExecutionPolicy Bypass -File scripts/smoke-prefs.ps1   # setting survives restart (launches the app itself)
```

The smoke test drives the real window through Windows UI Automation. It only types if the app is the foreground window, and otherwise skips that step and says so.

## Build an installer

```powershell
npm run tauri build      # NSIS installer in src-tauri/target/release/bundle/nsis/
```

The first build downloads the NSIS tools from Tauri's GitHub releases (hash-verified) into Tauri's cache. The installer is per-user and needs no admin rights. It is unsigned, so expect a SmartScreen warning.

## Set up listening

Talking to the assistant uses Microsoft Azure AI Speech (free F0 tier). See [docs/azure-speech-setup.md](docs/azure-speech-setup.md). Spoken replies use Windows' built-in voices and need no setup.

## Set up understanding

Understanding requests in your own words uses Google Gemini. See [docs/gemini-setup.md](docs/gemini-setup.md). Without a key the app still works with a few fixed phrases.

## Docs

- [ARCHITECTURE.md](ARCHITECTURE.md): modules, decisions, lessons from native testing
- [SECURITY.md](SECURITY.md): permissions model, risk levels, data handling
- [CHECKLIST.md](CHECKLIST.md): development progress and known limitations
