# Changelog

## Unreleased

- **Web pages: buttons and typing.** The assistant can search on websites and fill in ordinary boxes ("search this shop for milk", "add milk to the basket"). It types only after confirming the right box in the front browser window has the cursor, never presses Enter, and never types into password or payment boxes. Checkout, pay, place order, sign up and similar buttons ask for confirmation first.
- Requests may now take up to 8 assistant steps / 120 seconds for multi-step tasks.
- **Fixed: multi-step requests stopping after the first step** ("open YouTube and search for Yeat" only opened YouTube). The app now waits for pages to finish loading, recognises search boxes with suggestions (like YouTube's), falls back to the stronger `gemini-3.6-flash` before `gemini-3.5-flash-lite` when the main model is rate-limited, waits instead of giving up when rate-limited mid-task, and tells the assistant to finish every part of a request. Verified live: one request opened YouTube, typed Yeat, pressed Search and reported the results.

## 0.2.0 (2026-10-18)

First version that can be talked to in your own words and act in the browser.

- **Listening** with Microsoft Azure AI Speech (key in Windows Credential Manager; the app only ever handles 10-minute tokens).
- **Speaking** replies with Windows' built-in voices; Stop, Esc or the microphone silence them instantly.
- **Understanding** with Google Gemini (`gemini-3.8-flash`, falls back to `gemini-3.5-flash-lite` when busy), given the local date and time.
- **Computer actions:** open the browser; open https websites (checked from the address bar); read, scroll and follow links on web pages through Windows UI Automation.
- **Confirmation screen** before anything high-risk, built from the exact checked action; only an explicit Yes (button or clear spoken "yes") approves, once, within 2 minutes.
- **Practice mode** for trying the confirmation screen with a pretend message.
- **Safety:** every model-proposed action is validated in Rust (known tool, exact arguments, risk level); web page text is treated as untrusted; risky link texts need confirmation.
- **Reliability:** retries and model fallback, bounded requests, cancellation, plain-language errors, a crash screen, and a local redacted diagnostics log (Help).
- **Accessibility:** large controls, text sizes, high-contrast mode support, screen-reader labels, automated axe checks.
- New app icon; per-user NSIS installer.

## 0.1.0 (2026-10-17)

Foundation: Tauri 2 + React desktop app with an accessible main screen, typed and permission-checked native commands, saved text size, and the first real action (opening the default browser).
