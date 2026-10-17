import { type FormEvent, useEffect, useState } from "react";
import { type SpeechSetup, clearAzureSpeech, getSpeechSetup, saveAzureSpeech } from "../ipc/speech";
import { errorText } from "./errorText";

/** Azure Speech key setup. The key goes to Rust once and is never shown again. */
export function SpeechSettings({ onChanged }: { onChanged: () => void }) {
  const [setup, setSetup] = useState<SpeechSetup | null>(null);
  const [region, setRegion] = useState("");
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    getSpeechSetup()
      .then(setSetup)
      .catch((e) => setError(errorText(e, "I couldn't check the listening settings.")));
  }, []);

  async function onSave(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      setSetup(await saveAzureSpeech(region, key));
      setKey("");
      setNotice("Saved and tested. You can talk to me now.");
      onChanged();
    } catch (err) {
      setError(errorText(err, "That didn't work. Please check the key and region."));
    } finally {
      setBusy(false);
    }
  }

  async function onRemove() {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      setSetup(await clearAzureSpeech());
      setNotice("The key has been removed from this computer.");
      onChanged();
    } catch (err) {
      setError(errorText(err, "I couldn't remove the key."));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="speech-settings" aria-labelledby="speech-heading">
      <h3 id="speech-heading">Listening (Microsoft Azure Speech)</h3>
      <p className="hint-left">
        While the microphone button is on, your voice is sent to Microsoft to be turned into
        text. Nothing is recorded or kept by this app.
      </p>

      {setup?.configured ? (
        <>
          <p>
            <span aria-hidden="true">✓ </span>Set up, using region <strong>{setup.region}</strong>.
          </p>
          <button type="button" onClick={onRemove} disabled={busy}>
            Remove key from this computer
          </button>
        </>
      ) : (
        <form className="stacked-form" onSubmit={onSave}>
          <label htmlFor="azure-region">Azure region</label>
          <input
            id="azure-region"
            value={region}
            onChange={(e) => setRegion(e.target.value)}
            placeholder="for example: eastus"
            autoComplete="off"
            spellCheck={false}
            maxLength={40}
          />
          <label htmlFor="azure-key">Speech key</label>
          <input
            id="azure-key"
            type="password"
            value={key}
            onChange={(e) => setKey(e.target.value)}
            autoComplete="off"
            spellCheck={false}
            maxLength={128}
          />
          <button type="submit" disabled={busy || !region.trim() || !key.trim()}>
            {busy ? "Checking with Microsoft…" : "Save and test"}
          </button>
          <details>
            <summary>How do I get a key?</summary>
            <ol>
              <li>Sign in at portal.azure.com.</li>
              <li>
                Choose <strong>Create a resource</strong>, search for <strong>Speech</strong>, and
                create it.
              </li>
              <li>
                For <strong>Pricing tier</strong> pick <strong>Free F0</strong>. It includes 5 hours
                of listening a month and never charges you.
              </li>
              <li>
                When it's ready, open <strong>Keys and Endpoint</strong>. Copy{" "}
                <strong>Key 1</strong> and the <strong>Location/Region</strong> into the boxes
                above.
              </li>
            </ol>
          </details>
        </form>
      )}
      {error && <p role="alert">{error}</p>}
      {notice && <p role="status">{notice}</p>}
    </section>
  );
}
