# Privacy: what data goes where

This app is built so that you can tell exactly what leaves your computer and when.

## Stays on this computer

| What | Where | How long | How to remove |
|---|---|---|---|
| Settings (text size, read replies aloud, practice mode) | `%APPDATA%\com.voiceagent.desktop\voice-agent.db` | Until changed | Uninstall, or delete the file |
| Azure Speech key and region | Windows Credential Manager, `VoiceDesktopAgent/AzureSpeech` | Until removed | Settings → Listening → Remove key |
| Gemini key | Windows Credential Manager, `VoiceDesktopAgent/Gemini` | Until removed | Settings → Understanding → Remove key |
| Technical log (errors, status codes, which tools ran, timings) | `%APPDATA%\com.voiceagent.desktop\logs\diagnostics.log` (max ~256 KB + one older file) | Rolls over by size | Help → Clear them |
| The conversation on screen and the last 6 turns used as context | App memory only | Until the app closes | Close the app |

**Never stored:** audio recordings, screenshots, web page text, passwords or payment details, full conversation transcripts. The technical log is redacted before writing: keys, tokens, web addresses, email addresses and your Windows user name are replaced, and it never contains what you said or what a page said.

## Sent to Microsoft (Azure AI Speech): only while listening

- Your microphone audio, only while the microphone button says **Listening**.
- Used to turn speech into text. This app does not record or keep the audio.
- See Microsoft's data, privacy and security documentation for Azure AI Speech.

## Sent to Google (Gemini): only when you make a request

- What you said or typed, the last few turns of this session's conversation, the assistant's fixed instructions, the list of tools, and your computer's local date and time.
- **When you ask about a web page:** up to about 6,000 characters of that page's text. ("Read me this article" is different: that text is read aloud by your computer and is **not** sent to Google.)
- **When you ask what's on your screen:** the titles of your open windows.
- **When file search is on and you ask for a file:** the names, folder names, dates and sizes of up to 10 matching files (never their contents or full paths).
- Requests are sent with `store: false`, so Google doesn't keep them as stored interactions.
- **Free tier:** Google's pricing page says free-tier content may be used to improve Google's products. Turn on billing for the Gemini project before using the assistant with real users' personal requests.

## Never sent anywhere

Screenshots, files on your computer, your passwords, the contents of other windows, the technical log (unless you copy it yourself from Help).

## Things the app does on your computer

Only through the tools listed in SECURITY.md: open the browser, open https websites, read, scroll and follow links in the browser. Anything high-risk asks you on screen first.
