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

## Phase 3: Voice interface
- [ ] STT/TTS provider traits in Rust; visibly labelled mock provider
- [ ] Provider choice (after a docs, pricing and licensing review; needs approval if paid)
- [ ] Credential Manager storage and a configuration screen
- [ ] Mic permission flow and listening indicator

## Phase 4: First real computer tool ✅
- [x] Win32 helpers via the official `windows` crate: foreground window, process path, default https handler
- [x] `open_application` with an enum allowlist (default browser), low-risk gate, resolved .exe validation, launch without a shell, in-flight duplicate guard
- [x] Verification: the launched program's window must reach the foreground within 8 s; otherwise the app says it couldn't confirm
- [x] Fixed-phrase recogniser for typed requests (temporary until Phase 5) and a task runner that stays honest after Stop
- [x] Real desktop test: `scripts/smoke-open-browser.ps1` opened Edge, verified by the app and independently
- [ ] Foreground-window check enforced in Rust before any input simulation (lands with `type_text`)
- [ ] `get_active_window` exposed as an agent tool (internal only for now; the orchestrator arrives in Phase 5)

## Phases 5–8
See the master prompt §14. Not started.

## Known limitations
- No voice input or output and no reasoning model. The only computer action is opening the default browser, triggered by a few fixed typed phrases.
- If the browser was already in front, verification can't tell a new window from the existing one.
- The smoke test's typing step needs the app in the foreground (Windows foreground lock). Launching the app just before the test usually satisfies this.
- The installer is unsigned, so Windows SmartScreen will warn when it runs. Code signing comes in Phase 8.
