# Voice Desktop Agent

A Windows 11 desktop assistant you talk to like a phone call. It opens apps, reads the screen, and helps with everyday tasks, and it always asks before anything important.

> **Status: Phase 7.** Talk or type in your own words. Azure Speech listens, Google Gemini understands, Windows voices answer. It can open your browser and websites, read a page, scroll, follow links, search and fill in ordinary boxes, and press buttons, checking each step. Anything risky shows a confirmation screen first. It **cannot yet hear, speak, or control your computer**, and the UI says so. See [CHECKLIST.md](CHECKLIST.md).

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
# Pretend shop end-to-end: search, add to basket, checkout asks first (VISIBLE: opens a local page, types into it):
powershell -ExecutionPolicy Bypass -File scripts/smoke-shop.ps1
# Pretend webmail (VISIBLE: opens a local page, types into it, pretend Send; nothing is sent):
cargo test --manifest-path src-tauri/Cargo.toml live_mail -- --ignored --nocapture --test-threads=1
# Spotify (VISIBLE: opens Spotify and types 'Yeat' into its search box; nothing is played):
cargo test --manifest-path src-tauri/Cargo.toml live_spotify_controls -- --ignored --nocapture --test-threads=1
# Timer goes off in the real app; talk shortcut brings it forward and listens (needs Gemini + Listening; mic turns on briefly):
powershell -ExecutionPolicy Bypass -File scripts/smoke-helpers.ps1
# Built-in commands, no AI (VISIBLE: opens YouTube):
powershell -ExecutionPolicy Bypass -File scripts/smoke-built-in.ps1
# Natural voice: Azure returns audio for the stored key (nothing is played):
cargo test --manifest-path src-tauri/Cargo.toml live_tts -- --ignored --nocapture
# Wake word (AUDIBLE; needs the microphone to hear the speakers, which many laptops prevent):
cargo test --manifest-path src-tauri/Cargo.toml live_wake -- --ignored --nocapture --test-threads=1
# Confirmation gate in Practice mode (needs a Gemini key; launches the app itself; nothing is sent):
powershell -ExecutionPolicy Bypass -File scripts/smoke-confirm.ps1
# Sends a deliberately FAKE key to Microsoft and checks it is rejected and not stored:
powershell -ExecutionPolicy Bypass -File scripts/smoke-speech-setup.ps1
# close the app, then:
powershell -ExecutionPolicy Bypass -File scripts/smoke-prefs.ps1   # setting survives restart (launches the app itself)
```

The smoke test drives the real window through Windows UI Automation. It only types if the app is the foreground window, and otherwise skips that step and says so.

## Troubleshooting

Help → **Something not working?** → **Show technical details** shows the app's recent technical log (redacted; never your conversation). Copy it into a message to whoever is helping you.

## Release procedure

1. Update the version in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`, and add a CHANGELOG entry.
2. `npm test`, `npm run test:rust`, `npm run build`.
3. `npm run tauri build` → `src-tauri/target/release/bundle/nsis/Voice Desktop Agent_<version>_x64-setup.exe`.
4. Run the desktop smoke tests against the release build (see Test).
5. The app icon is generated from `app-icon.svg` with `npx tauri icon app-icon.svg` (then delete the `android/` and `ios/` folders it creates).

## Build an installer

```powershell
npm run tauri build      # NSIS installer in src-tauri/target/release/bundle/nsis/
```

The first build downloads the NSIS tools from Tauri's GitHub releases (hash-verified) into Tauri's cache. The installer is per-user and needs no admin rights. It is unsigned, so expect a SmartScreen warning.

## Set up listening

Talking to the assistant uses Microsoft Azure AI Speech (free F0 tier). See [docs/azure-speech-setup.md](docs/azure-speech-setup.md). Spoken replies use Windows' built-in voices and need no setup.

## Practice mode

Settings → **Practice mode** lets you try an important action (sending a pretend message) and see the confirmation screen. Nothing is ever really sent, and the app says so on screen.

## Set up understanding

Understanding requests in your own words uses Google Gemini. See [docs/gemini-setup.md](docs/gemini-setup.md). Without a key the app still works with a few fixed phrases.

## Docs

- [ARCHITECTURE.md](ARCHITECTURE.md): modules, decisions, lessons from native testing
- [SECURITY.md](SECURITY.md): permissions model, risk levels, data handling
- [CHECKLIST.md](CHECKLIST.md): development progress and known limitations
- [PRIVACY.md](PRIVACY.md): exactly what data stays local and what goes to Microsoft or Google
- [CHANGELOG.md](CHANGELOG.md): what changed in each version
