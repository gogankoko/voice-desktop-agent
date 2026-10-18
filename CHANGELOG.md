# Changelog

## Unreleased

- **Built-in commands: instant, free, offline.** Everyday requests are now handled by the app itself, without asking the AI: "open YouTube" (and ~35 other popular sites), "open Spotify", "open my browser", "pause", "next song", "turn it up", "set a timer for 10 minutes", "remind me in 20 minutes to take my pills", "how much battery do I have?", "minimize this", "what time is it?". They answer straight away, work without a Gemini key, and don't use any Gemini quota. Asking for something specific, like "play Yeat on Spotify", still goes to the AI.
- **Talk from anywhere:** press Ctrl+Alt+Space (or, if another program already uses it, the shortcut shown in the app) to bring the assistant forward and start listening. It also answers its yes/no questions.
- **Timers and reminders:** "remind me in 20 minutes to take my pills", "set a timer for 10 minutes", "remind me at 5 to call Anna", "what reminders do I have?". They chime, appear and are read aloud, and bring the app back if it was minimized. They only work while the app is open.
- **Window control:** "switch to Spotify", "minimize this", "make it full screen", "close Notepad" (closing asks first).
- **Computer status:** "how much battery do I have?", "am I online?", "how much space is left?".
- **Open and operate your apps:** "open Spotify", "play Yeat on Spotify", "pause the music", "next song", "turn it up". The assistant can open any app in your Start menu by name, press buttons and type into search boxes inside it, and use the media keys. Command lines, system tools, installers and password managers are always off limits; buttons like remove, delete, send or buy ask you first.
- **Make everything bigger:** the assistant can turn on Windows Magnifier and zoom in or out, zoom web pages, and open the right Windows Settings page for bigger text, display size or a bigger mouse pointer.
- **Find your files (opt-in):** "find the photo I downloaded yesterday" searches file names in Downloads, Pictures, Documents and Desktop, then opens the photo or shows it in its folder. Off until you turn it on in Settings.
- **Open everyday apps:** Notepad, Calculator, Paint, File Explorer and Windows Settings, as well as the browser.
- **Explain this error message:** the assistant can read the text and buttons of any open window when you ask (never password boxes) and explain it in plain words.
- **Help me write an email:** the assistant can fill in the To, Subject and Message boxes on your webmail page. Sending always asks first, and the confirmation screen now shows what's filled in on the page (recipient, subject and message), so you can check before saying yes.

- **Step-by-step progress:** the status line shows what's happening ("Opening youtube.com", "Typing 'Yeat' into 'Search'").
- **Pause / Continue** a request while it's working.
- **Undo:** "undo that" goes back a page or puts back what was in a box; it says honestly when something can't be undone.
- **Read aloud:** "read me this article" reads the page in full with the Windows voice, with Pause / Continue / Go back a bit. The article text is not sent to Google.
- **What's on my screen:** the assistant can list the open windows (only when asked).
- Fixed an occasional error inside the speech library when listening ended.

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
