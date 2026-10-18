# Setting up understanding (Google Gemini)

With a Gemini key the assistant understands requests in your own words ("can you get the
internet up for me?") instead of a few fixed phrases. It still only does what the app's
built-in tools allow; right now that is opening the web browser.

## 1. Get a key (free)
1. Go to https://aistudio.google.com and sign in with a Google account.
2. Choose **Get API key**, then **Create API key**. If asked, pick or create a Google Cloud project.
3. Copy the key. It usually starts with `AIza`.

## 2. Enter it in the app
Settings → **Understanding requests (Google Gemini)** → paste the key → **Save and test**.
The app checks the key with a free call to Google before saving it.

## Cost and the free tier
- The app uses **gemini-3.8-flash**. It has a free tier with daily limits, enough for testing.
- On the **free tier, Google may use your requests to improve its products** (Google's pricing
  page, October 2026). For real users, turn on billing for the project in Google AI Studio.
  Paid requests are not used that way.
- Paid price (October 2026): about $0.75 per million input tokens and $3.75 per million output
  tokens, roughly 0.3 cents per request. Google's page says this doubles from 2027-01-01.
  Set a budget alert in Google Cloud if you turn on billing.

## Privacy
- The app sends: what you said or typed, the last few turns of this session's conversation,
  the assistant's instructions, and the list of tools. Nothing from your screen or files.
- Requests are sent with `store: false`, so Google does not keep them as stored interactions.
- The conversation lives only in the app's memory and is forgotten when the app closes.
- The key is stored in **Windows Credential Manager** (`VoiceDesktopAgent/Gemini`) and never
  appears in the app's interface or logs. **Remove key from this computer** deletes it.

## Troubleshooting
| Message | What to do |
|---|---|
| "Google didn't accept that key" | Copy it again from AI Studio; make sure nothing extra was copied. |
| "The assistant is busy, or today's free limit has been reached" | Wait a minute, or turn on billing. |
| It does only the first part of a request | Usually the free tier's 5-requests-per-minute limit: a task like "open YouTube and search for X" needs 4-6. The app switches to backup models, but turning on billing makes multi-step tasks reliable. |
| "I couldn't reach the assistant service" | Check the internet connection, VPN or firewall. |
| It says it can't do something | Correct: only opening the browser exists so far. More tools come in later phases. |
