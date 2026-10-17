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
- [x] Release build and NSIS installer built (2.07 MB per-user installer; install itself not yet tried)
- [x] Text-size preference persisted in SQLite (`prefs.rs`), verified across restart (`scripts/smoke-prefs.ps1`)
- [x] Text size uses radio buttons (all options visible, drivable via UIA SelectionItem)
- [x] `.gitattributes` for consistent line endings
- [ ] Component tests (jsdom + Testing Library), moved to Phase 8 hardening

## Phase 3: Voice interface
- [ ] STT/TTS provider traits in Rust; visibly labelled mock provider
- [ ] Provider choice (after a docs, pricing and licensing review; needs approval if paid)
- [ ] Credential Manager storage and a configuration screen
- [ ] Mic permission flow and listening indicator

## Phase 4: First real computer tool
- [ ] `get_active_window` via the `windows` crate
- [ ] `open_application` against an allowlist (default browser first), with verification
- [ ] Foreground-window check enforced before any input simulation

## Phases 5–8
See the master prompt §14. Not started.

## Known limitations
- No voice input or output, no reasoning model, no computer control yet.
- The smoke test's typing step needs the app in the foreground (Windows foreground lock). Launching the app just before the test usually satisfies this.
- The installer is unsigned, so Windows SmartScreen will warn when it runs. Code signing comes in Phase 8.
