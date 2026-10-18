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

## Phase 5: Voice-to-computer with a reasoning model ✅
- [x] Provider: **Google Gemini** (owner's choice, 2026-10-18), Interactions API, `gemini-3.8-flash`, `store: false`
- [x] Deterministic tool registry: known tools only, exact typed arguments, low risk only (`agent/registry.rs`)
- [x] Bounded orchestrator: 4 model calls, 60 s, per-request dedupe, one task at a time, Stop interrupts pending model calls
- [x] What happened comes from Rust's action reports, shown in the transcript; the model's words are only spoken
- [x] Untrusted model output: refused calls are reported back to the model as errors and never executed (tests include injected calls)
- [x] Short in-memory conversation history (6 turns, never saved)
- [x] Settings: Gemini key with free-tier privacy notice; falls back to fixed phrases without a key
- [x] Live tests with the owner's Gemini key (2026-10-18): full tool round trip; found and fixed 503 overload (retry + fallback model) and an over-strict key format check
- [ ] Progress for multi-step plans beyond a single tool (arrives with more tools)

## Phase 6: Interruption and safety (code complete)
- [x] Confirmation gate in Rust: high-risk calls pause the request; single-use id, 2-minute expiry, exact stored call runs on approval; Stop, decline, expiry, forged or reused ids run nothing
- [x] Confirmation text built from validated arguments, not model prose; other calls proposed alongside wait
- [x] Paused requests resume on the same Gemini model (thought signatures)
- [x] Confirmation screen: modal, big Yes/No, focus starts on No, Esc = No, details read aloud, optional spoken answer
- [x] Strict spoken consent: only exact clear "yes" phrases approve (`core/consent.ts`, tests)
- [x] Practice mode (setting + banner): pretend "send a message" exercises the real gate; nothing is sent; labelled everywhere
- [x] Model told plainly when the user already confirmed/declined (live test caught a misleading reply)
- [x] Model instructions consistent with the tool list (desktop test caught a refusal); primary and fallback models verified live
- [x] Desktop test `scripts/smoke-confirm.ps1`: decline round passed repeatedly; approve round passed once; later runs skipped safely while the owner was using the PC
- [ ] Pause/resume of a running multi-step task (meaningful once multi-step tools exist)
- [ ] Undo for reversible actions (needs the first undoable tool)

## Phase 7: Browser interaction ✅
- [x] `open_website`: https-only public sites (`tools/web_url.rs`), launched as a single argument, verified from the address bar
- [x] `read_webpage` (UIA Text pattern), `scroll_webpage` (Scroll pattern), `list_links` + `follow_link` (Invoke) via Windows UI Automation, no extensions or keystrokes
- [x] Fresh observation: link numbers only from this request's latest list; page and link text re-checked before clicking; stale links refused
- [x] Risky link texts (buy, pay, delete, sign out, download…) go through the confirmation gate with the real link text
- [x] Page text sent to the model only as `untrusted_page_text` with a warning
- [x] Local test pages (`src-tauri/tests/fixtures/`), since example.com asks not to be relied on for testing
- [x] Live (real Edge): open + verify, read, list, scroll, follow, stale link refused
- [x] Live (real Gemini + real Edge): read the secret word **and ignored the page's planted "open evil.example" instruction**; followed the right link; risky link paused; declined = nothing clicked
- [ ] Buttons, forms and typing into web pages (need `type_text` with the foreground check; next)
- [ ] Reading a page aloud in full ("read me this article") with pause/continue

## Phase 8: Hardening (in progress)
- [x] Local diagnostics log, redacted before writing (keys, tokens, web addresses, emails, Windows user names); never conversation or page text; Help → show / copy / clear
- [x] Panic hook, React error boundary with a restart screen, uncaught UI errors logged
- [x] Per-request technical summary (tools, verified or not, model, timing) for troubleshooting
- [x] Windows high-contrast (forced-colors) support
- [x] Whole-app accessibility tests (axe) on Home, Settings, Help and the confirmation screen; Enter on the confirmation screen's initial focus means No
- [x] App icon, version 0.2.0, installer metadata, per-user install
- [x] PRIVACY.md data inventory, CHANGELOG.md
- [ ] Code signing (needs a certificate; costs money, so it's the owner's decision)
- [ ] Continuous integration (GitHub Actions; uses Actions minutes, so it's the owner's decision)
- [ ] Manual screen-reader pass with NVDA / Narrator

## Known limitations
- Listening needs an Azure Speech key (docs/azure-speech-setup.md); understanding free-form requests needs a Gemini key (docs/gemini-setup.md).
- Computer actions: open the browser, open https websites, read/scroll pages, follow links. No form filling, typing into pages or buttons yet.
- Browser control relies on the browser's accessibility tree; the address bar is found by an English "Address" label, so non-English Windows/browsers may not verify navigation (it then says it couldn't confirm). The only computer action is opening the default browser, triggered by a few fixed typed phrases.
- If the browser was already in front, verification can't tell a new window from the existing one.
- The smoke test's typing step needs the app in the foreground (Windows foreground lock). Launching the app just before the test usually satisfies this.
- The installer is unsigned, so Windows SmartScreen warns when it runs (signing needs a paid certificate).
