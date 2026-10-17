# Security and permissions model

## Principles

1. Safety decisions are made in deterministic code (Rust), never only in model prompts.
2. Model output, web pages, documents, emails and screenshots are **untrusted data**. Instructions inside them never override the user, tool permissions or this policy.
3. Least privilege everywhere: Tauri capabilities, native commands, file scopes.
4. Nothing is recorded, observed or sent without a visible indicator and the user's action.

## Tauri boundary (in force now)

- No `opener`, `shell`, `fs`, `http` or `process` plugins.
- App commands must be listed in `src-tauri/build.rs` **and** granted in `src-tauri/capabilities/default.json`. Currently granted: `get_app_status` (read-only), `get_preferences`, `set_preferences` (enum-typed, `deny_unknown_fields`), `get_speech_setup`, `save_azure_speech` (validated region and key, tested against Azure before storing, key never returned), `clear_azure_speech`, `get_speech_token` (10-minute Azure STS token, cached 9 minutes), `open_application` (enum allowlist: `default_browser` only; Rust resolves and validates the executable from the Windows https association and launches it without a shell or arguments; refuses non-low-risk apps until confirmations exist; one launch at a time).
- CSP: `default-src 'self'`, no remote scripts, frames or form posts. Beyond IPC, `connect-src` allows only `wss://*.stt.speech.microsoft.com`. The Speech SDK's audio worklet is served from the app (`public/speech-processor.js`) instead of a `blob:` URL. Prototype freezing is on.
- All IPC responses are validated with zod in `src/ipc/*`. Inputs to future commands are validated in Rust.

## Action risk levels (enforced by the planned tool registry)

| Level | Examples | Rule |
|---|---|---|
| Low | open an ordinary app, navigate, scroll, read the screen | May run on its own; announced. |
| Medium | edit a document, fill a form without submitting, download, change a preference | Runs with a clear spoken summary; undo offered where possible. |
| High | purchase, send a message or email, submit a consequential form, delete data, share private info, move money, change security settings, install software | **Explicit confirmation immediately before each action**, showing exact details. A fresh confirmation id each time. Never blanket or prior consent. |

Silence, background speech, ambiguous replies and partial utterances are never consent (enforced by the state machine; see tests).

## Forbidden to the agent

Arbitrary shell or PowerShell, running generated code, unrestricted JavaScript in pages, bypassing CAPTCHAs, authentication or security warnings, revealing credentials, hidden remote access.

## Data handling

- Raw audio, full screenshots, passwords, payment data and full transcripts are not stored by default.
- Provider API keys go in Windows Credential Manager (`VoiceDesktopAgent/AzureSpeech`). Never in the frontend bundle, committed config, test fixtures or logs. Rust types holding keys or tokens redact them in `Debug` output. The frontend receives only short-lived tokens.
- Microphone audio goes to Azure only while the mic button shows Listening. It is not recorded or stored. Silence or unclear speech is never treated as a request or as consent.
- Logs are redacted for sensitive fields.
- Mock modes are visibly labelled and never claim to have acted on the real computer.

## Input simulation

Before any keystroke or click is sent, the target window must be the OS foreground window (`GetForegroundWindow`) and the target element must be re-observed. If the check fails the action is refused, never "tried anyway". See ARCHITECTURE.md for why element focus alone is insufficient.

## Reporting

This is a pre-release project; report issues to the repository owner.
