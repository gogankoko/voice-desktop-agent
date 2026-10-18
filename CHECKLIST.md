# Development checklist

## Phase 1: Environment and architecture ✅
- [x] Toolchain verified (Node 24.22, npm 11.19, Rust 1.99 MSVC, VS Build Tools 2026, SDK 10.0.26100, WebView2 155)
- [x] ARCHITECTURE.md, SECURITY.md, README.md

## Phase 2: Desktop foundation ✅
- [x] Tauri 2.12 + React 19 + TS + Vite 8 scaffold, opener plugin removed
- [x] Strict CSP, minimal capabilities, app-command manifest
- [x] `get_app_status` command, zod-validated on the frontend
- [x] Conversation state machine with safety invariants (15 tests)
- [x] Accessible main screen: large mic button (disabled, explained), status with text and symbol, transcript, text input, Help, Settings (text size, capability report), persistent Stop + Esc
- [x] Desktop smoke test via UI Automation (`scripts/smoke-test.ps1`)
- [x] Release build and NSIS installer verified (installed per-user to %LOCALAPPDATA%\Voice Desktop Agent and ran)
- [x] Text-size preference persisted in SQLite (`prefs.rs`), verified across restart (`scripts/smoke-prefs.ps1`)
- [x] Text size uses radio buttons (all options visible, drivable via UIA SelectionItem)
- [x] `.gitattributes` for consistent line endings
- [ ] Component tests (jsdom + Testing Library), moved to Phase 8 hardening

## Phase 3: Voice interface ✅
- [x] Provider review (2026-10-18): Windows built-in speech recognition needs MSIX package identity, so it's not usable with our installer. Owner chose **Azure AI Speech** (F0 free tier), English
- [x] Provider-independent `SpeechRecognizer` / `SpeechSynthesizer` interfaces with mocks (`src/voice/`)
- [x] Azure key in Windows Credential Manager; tested against Azure before saving; frontend only gets 10-minute tokens
- [x] Settings: Azure setup with instructions, remove key, "Read my replies aloud"
- [x] Spoken replies with on-device Windows voices; Stop, Esc and mic barge-in silence them
- [x] Listening indicator: button text, symbol, ring, status line, live words; mic only on while "Listening"
- [x] SDK audio processor served from the app (no CSP loosening)
- [x] Fake-key rejection test: `scripts/smoke-speech-setup.ps1` (needs the app in the foreground)
- [x] Real voice test with the owner's Azure key and microphone (2026-10-18: spoke "open my browser", Edge opened)
- [ ] Mock providers exposed as a visible "practice mode" in the UI (currently tests only)

## Phase 4: First real computer tool ✅
- [x] Win32 helpers via the official `windows` crate: foreground window, process path, default https handler
- [x] `open_application` with an enum allowlist (default browser), low-risk gate, resolved .exe validation, launch without a shell, in-flight duplicate guard
- [x] Verification: the launched program's window must reach the foreground within 8 s; otherwise the app says it couldn't confirm
- [x] Fixed-phrase recogniser for typed requests (temporary until Phase 5) and a task runner that stays honest after Stop
- [x] Real desktop test: `scripts/smoke-open-browser.ps1` opened Edge, verified by the app and independently
- [ ] Foreground-window check enforced in Rust before any input simulation (lands with `type_text`)
- [ ] `get_active_window` exposed as an agent tool (internal only for now; the orchestrator arrives in Phase 5)

## Phase 5: Voice-to-computer with a reasoning model (code complete; live test pending a Gemini key)
- [x] Provider: **Google Gemini** (owner's choice, 2026-10-18), Interactions API, `gemini-3.8-flash`, `store: false`
- [x] Deterministic tool registry: known tools only, exact typed arguments, low risk only (`agent/registry.rs`)
- [x] Bounded orchestrator: 4 model calls, 60 s, per-request dedupe, one task at a time, Stop interrupts pending model calls
- [x] What happened comes from Rust's action reports, shown in the transcript; the model's words are only spoken
- [x] Untrusted model output: refused calls are reported back to the model as errors and never executed (tests include injected calls)
- [x] Short in-memory conversation history (6 turns, never saved)
- [x] Settings: Gemini key with free-tier privacy notice; falls back to fixed phrases without a key
- [ ] **Live test with the owner's Gemini key**
- [ ] Progress for multi-step plans beyond a single tool (arrives with more tools)

## Phases 6–8
See the master prompt §14. Not started.

## Known limitations
- Listening needs an Azure Speech key (docs/azure-speech-setup.md); understanding free-form requests needs a Gemini key (docs/gemini-setup.md).
- The only computer action is still opening the default browser. No confirmation dialog yet, so only low-risk tools can exist. The only computer action is opening the default browser, triggered by a few fixed typed phrases.
- If the browser was already in front, verification can't tell a new window from the existing one.
- The smoke test's typing step needs the app in the foreground (Windows foreground lock). Launching the app just before the test usually satisfies this.
- The installer is unsigned, so Windows SmartScreen will warn when it runs. Code signing comes in Phase 8.
