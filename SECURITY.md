# Security and permissions model

## Principles

1. Safety decisions are made in deterministic code (Rust), never only in model prompts.
2. Model output, web pages, documents, emails and screenshots are **untrusted data**. Instructions inside them never override the user, tool permissions or this policy.
3. Least privilege everywhere: Tauri capabilities, native commands, file scopes.
4. Nothing is recorded, observed or sent without a visible indicator and the user's action.

## Tauri boundary (in force now)

- No `opener`, `shell`, `fs`, `http` or `process` plugins.
- App commands must be listed in `src-tauri/build.rs` **and** granted in `src-tauri/capabilities/default.json`. Currently granted: `get_app_status` (read-only).
- CSP: `default-src 'self'`, no remote scripts, frames or form posts. Prototype freezing is on.
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
- Provider API keys go in Windows Credential Manager. Never in the frontend bundle, committed config, test fixtures or logs.
- Logs are redacted for sensitive fields.
- Mock modes are visibly labelled and never claim to have acted on the real computer.

## Input simulation

Before any keystroke or click is sent, the target window must be the OS foreground window (`GetForegroundWindow`) and the target element must be re-observed. If the check fails the action is refused, never "tried anyway". See ARCHITECTURE.md for why element focus alone is insufficient.

## Reporting

This is a pre-release project; report issues to the repository owner.
