# Architecture

Voice-first Windows 11 assistant. Tauri 2 shell, Rust native layer, React/TypeScript UI.
No local HTTP server: the UI talks to Rust only through Tauri commands and events.

```
┌──────────────────────────── WebView2 (untrusted-ish UI) ────────────────────────────┐
│ React UI ── conversation state machine (src/core/conversation) ── typed IPC (src/ipc)│
└───────────────────────────────────────┬─────────────────────────────────────────────┘
                                        │ Tauri commands (each listed in build.rs AND
                                        │ granted in capabilities/default.json)
┌───────────────────────────────────────▼─────────────────────────────────────────────┐
│ Rust native layer (src-tauri)                                                        │
│  commands.rs   thin handlers: validate → delegate → serialise                        │
│  status.rs     honest capability report                                              │
│  prefs.rs      SQLite (rusqlite, bundled) preferences in %APPDATA%\com.voiceagent…   │
│  [planned] agent/      orchestrator: plan, risk-classify, confirm, execute, verify   │
│  tools/        risk levels; open_app.rs (allowlisted launch + verify); win.rs (Win32)│
│  tools/browser.rs  UI Automation: open+verify, read, scroll, list/follow links       │
│  tools/web_url.rs  https-only URL validation                                         │
│  agent/        mod.rs orchestrator (bounded loop, cancel, dedupe, history)           │
│                gemini.rs Interactions API client · registry.rs tool gate · prompt.rs │
│  speech.rs     Azure key validation, STS token issue + cache                         │
│  secrets.rs    Windows Credential Manager (generic credentials)                      │
│  [planned] store/      SQLite: preferences, optional task summaries                  │
└──────────────────────────────────────────────────────────────────────────────────────┘
```

## Key decisions

| Decision | Reason |
|---|---|
| Tauri **2.x** pinned (resolved 2.12.4) | crates.io also publishes 3.0 alphas; the spec targets 2 stable. |
| No opener / shell / fs plugins | Each would hand the WebView broad OS power. Native actions go through our own narrow commands. |
| App-command manifest in `build.rs` | Without it, Tauri allows every registered app command by default. With it, each command needs an explicit capability grant. |
| Agent orchestration and risk policy will live in **Rust**, not the UI | Security decisions must be deterministic and outside the WebView. The UI renders state and collects confirmations. |
| Conversation state machine is pure TS | Fully unit-testable; the UI mirrors it. Once orchestration moves to Rust, Rust becomes the source of truth and emits state events. |
| Strict CSP, `freezePrototype` | Limits damage if untrusted content ever reaches the DOM. |
| NSIS bundle only | Per-user installer, no admin rights needed. |

## Voice layer (src/voice)

`SpeechRecognizer` and `SpeechSynthesizer` interfaces; the conversation code (`listenTurn`, `runRequest`) depends only on these.

- **Listening: Azure AI Speech** via the official JS Speech SDK in the WebView, authenticated with a 10-minute token from Rust (`get_speech_token`), so the resource key stays in Rust. One utterance per press (`recognizeOnceAsync`), live interim words, 8 s initial / 1.5 s end silence. Lazy-loaded.
- **Speaking: Windows voices** via WebView2 `speechSynthesis`: free, on-device, prefers a "Natural" en-US voice. Cancelled by Stop, Esc, the mic button (barge-in) and new typed requests.
- Why not Windows' own speech recognition: `Windows.Media.SpeechRecognition` requires MSIX package identity (Microsoft Learn, 2026); our NSIS build is unpackaged. Revisit if we move to MSIX.
- Mocks (`src/voice/mock.ts`) drive deterministic tests.

## Agent layer (src-tauri/src/agent)

```
user words ─► run_agent ─► Gemini (store:false) ─► proposed calls ─► registry::validate
                                ▲                                         │ ok + low risk
                                └──── function_result (verified, errors) ◄┴─ ToolExecutor (open_app)
```

- `ChatModel` trait: Gemini today; any provider that can propose function calls fits. Tests use a scripted model.
- `ToolExecutor` trait: `DesktopExecutor` for real, a fake in tests.
- Model steps (including thought signatures) are replayed verbatim within a request, as Google requires. Across requests only text turns are kept.
- The frontend (`core/agentTurn.ts`) maps progress events and the outcome onto the conversation state machine. Built-in commands (`src-tauri/src/agent/local.rs`) are matched in Rust before the AI and work without a Gemini key.
- **Confirmation (Phase 6):** `TaskRun` is resumable. `drive()` returns `Finished`, `Cancelled` or `NeedsConfirmation`; `AgentState` stores at most one paused run with a single-use id and the model that produced its steps; `confirm()` resolves the stored call (run or declined) and drives on. The UI shows `ConfirmDialog` and answers via `confirm_agent`.

```
run_agent ─► drive ─► high-risk call? ──yes──► pause (id, exact call, model) ─► UI: ConfirmDialog
                                                                        │ yes/no (button or strict voice)
                                     ◄── drive ◄── resolve(run | decline) ◄── confirm_agent(id)
```

## Conversation states

`idle → listening → transcribing → understanding → planning → (awaiting_confirmation) → executing → verifying → speaking → idle`,
plus `paused`, `recovering`, `cancelled`. See `src/core/conversation/machine.ts`. Invariants (tested):

- Only `CONFIRMED` with the matching confirmation id leaves `awaiting_confirmation`. Speech, silence, stale or duplicate confirmations cannot.
- `STOP` works from every active phase. Nothing stays pending after it.
- Events invalid for the current phase are ignored, so retries and duplicates can't advance a task twice.

## Computer-control lessons already learned (from the real smoke test)

- **WebView2 `HasKeyboardFocus` is not where keystrokes go.** It reflects DOM focus inside the WebView even when another app is in the foreground. `type_text` must check `GetForegroundWindow() == target HWND` **and** element focus immediately before sending input, and refuse otherwise.
- **UIA `ValuePattern.SetValue` doesn't trigger React `onChange`** in WebView2. For web content, prefer real keyboard input (after the foreground check), then re-read the value to verify.
- A `<select>` in WebView2 exposes only Value + ExpandCollapse, and its options never appear in the UIA tree when expanded from a background process. Radio buttons expose SelectionItem and work. The tool layer should expect many web dropdowns to be uncontrollable via UIA and fall back to keyboard (foreground-checked) or report "can't do this one".
- Chromium (Edge) exposes the page as a `Document` element with Text and Scroll patterns, links as `Hyperlink` with Invoke, and the address bar as an `Edit` whose name contains "Address". Reading, scrolling and clicking links all work without the browser being in front and without keystrokes.
- Identifying a link again: `(text, ordinal among links with that text, page address)`. Simpler than UIA runtime IDs and doubles as the "has anything changed?" check.
- Buttons with `aria-pressed` are exposed as toggle buttons (TogglePattern, no InvokePattern). The tool layer must handle Invoke, Toggle and SelectionItem patterns.

## Planned native dependencies (to be verified before adding)

- `windows` crate 0.62 (Microsoft, MIT/Apache-2.0): foreground window, process path, file associations; UI Automation next. Preferred over third-party `uiautomation` wrappers so we depend on the official bindings. **Added in Phase 4.**
- Credential storage: `keyring` crate with the Windows Credential Manager backend, or direct `windows` Credential APIs.
- HTTP: `reqwest` 0.13 with `native-tls` (Windows SChannel; avoids the aws-lc-rs C toolchain). **Added in Phase 3.**
- Speech: `microsoft-cognitiveservices-speech-sdk` 1.52 (MIT). **Added in Phase 3.**
- SQLite: `rusqlite` 0.40 with the bundled feature (MIT; SQLite is public domain). **Added in Phase 2.**
- Speech providers: chosen in Phase 3 after checking current docs, pricing and streaming support. Adding a paid provider needs owner approval.
