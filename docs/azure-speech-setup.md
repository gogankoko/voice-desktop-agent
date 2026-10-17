# Setting up listening (Azure AI Speech)

The assistant turns your voice into text with Microsoft Azure AI Speech. You need your own
free Azure Speech resource. The **Free (F0)** tier includes 5 hours of listening per month
and never charges: when the hours run out, listening stops until the next month.

## 1. Create the Speech resource
1. Sign in at https://portal.azure.com (create a free account if you don't have one).
2. Choose **Create a resource**, search for **Speech**, and select **Create**.
3. Choose a **Resource group** (create one, e.g. `voice-assistant`).
4. **Region**: pick one near you, e.g. `westeurope` or `eastus`.
5. **Pricing tier**: **Free F0**. (Only one F0 Speech resource is allowed per subscription.)
6. **Review + create**, then **Create**. Wait for "Your deployment is complete".

## 2. Copy the key and region
Open the resource, then **Keys and Endpoint** (under Resource Management). Copy:
- **KEY 1**
- **Location/Region**, e.g. `westeurope`

## 3. Enter them in the app
Settings → **Listening (Microsoft Azure Speech)** → paste the region and key → **Save and test**.
The app checks the key with Microsoft before saving it. On success the microphone button
becomes active.

## Where the key goes
- It is stored in **Windows Credential Manager** (Control Panel → Credential Manager →
  Windows Credentials → `VoiceDesktopAgent/AzureSpeech`), protected by your Windows account.
- It is never written to files, logs, or the app's interface.
- The app exchanges it for a 10-minute access token, and only that token is used to listen.
- **Remove key from this computer** in Settings deletes it.

## Privacy
Audio is sent to Microsoft only while the microphone button says **Listening**. This app
does not record or keep audio. See Microsoft's data, privacy and security notes for Azure
AI Speech for how Microsoft handles it.

## Troubleshooting
| Message | What to do |
|---|---|
| "Microsoft didn't accept that key for that region" | Check that both came from the same resource; try Key 2. |
| "I'm not allowed to use the microphone" | Windows Settings → Privacy & security → Microphone: turn on **Microphone access** and **Let desktop apps access your microphone**. |
| "I couldn't find a microphone" | Plug one in, or check Sound settings → Input. |
| "I couldn't reach Microsoft's speech service" | Check the internet connection, VPN or firewall. |
| Listening stops working near month end | The F0 free hours are used up; it resets next month. |
