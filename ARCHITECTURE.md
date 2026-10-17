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
│  [planned] tools/      typed tool registry, one module per tool, risk level per tool │
│  [planned] uia/        Windows UI Automation (windows crate)                         │
│  [planned] voice/      STT/TTS provider traits + mock + real adapters                │
│  [planned] secrets/    Windows Credential Manager                                    │
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
- Buttons with `aria-pressed` are exposed as toggle buttons (TogglePattern, no InvokePattern). The tool layer must handle Invoke, Toggle and SelectionItem patterns.

## Planned native dependencies (to be verified before adding)

- `windows` crate (Microsoft, MIT/Apache-2.0): UI Automation, foreground window, process launch. Preferred over third-party `uiautomation` wrappers so we depend on the official bindings.
- Credential storage: `keyring` crate with the Windows Credential Manager backend, or direct `windows` Credential APIs.
- SQLite: `rusqlite` 0.40 with the bundled feature (MIT; SQLite is public domain). **Added in Phase 2.**
- Speech providers: chosen in Phase 3 after checking current docs, pricing and streaming support. Adding a paid provider needs owner approval.
