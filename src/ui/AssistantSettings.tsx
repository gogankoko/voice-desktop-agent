import { type FormEvent, useEffect, useState } from "react";
import { type AssistantSetup, clearGeminiKey, getAssistantSetup, saveGeminiKey } from "../ipc/agent";
import { errorText } from "./errorText";

/** Gemini key setup. The key goes to Rust once and is never shown again. */
export function AssistantSettings({ onChanged }: { onChanged: () => void }) {
  const [setup, setSetup] = useState<AssistantSetup | null>(null);
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    getAssistantSetup()
      .then(setSetup)
      .catch((e) => setError(errorText(e, "I couldn't check the assistant settings.")));
  }, []);

  async function act(fn: () => Promise<AssistantSetup>, ok: string, fail: string) {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      setSetup(await fn());
      setKey("");
      setNotice(ok);
      onChanged();
    } catch (err) {
      setError(errorText(err, fail));
    } finally {
      setBusy(false);
    }
  }

  function onSave(e: FormEvent) {
    e.preventDefault();
    void act(
      () => saveGeminiKey(key),
      "Saved and tested. You can now ask me things in your own words.",
      "That didn't work. Please check the key.",
    );
  }

  return (
    <section className="speech-settings" aria-labelledby="assistant-heading">
      <h3 id="assistant-heading">Understanding requests (Google Gemini)</h3>
      <p className="hint-left">
        Your requests (what you say or type) are sent to Google to be understood. When you ask
        about a web page, its text is sent too. Google does not keep them between requests. On Google's free tier, Google may use requests to
        improve its products; turning on billing in Google AI Studio stops that.
      </p>

      {setup?.configured ? (
        <>
          <p>
            <span aria-hidden="true">✓ </span>Set up, using <strong>{setup.model}</strong>.
          </p>
          <button
            type="button"
            disabled={busy}
            onClick={() =>
              void act(clearGeminiKey, "The key has been removed from this computer.", "I couldn't remove the key.")
            }
          >
            Remove key from this computer
          </button>
        </>
      ) : (
        <form className="stacked-form" onSubmit={onSave}>
          <label htmlFor="gemini-key">Gemini API key</label>
          <input
            id="gemini-key"
            type="password"
            value={key}
            onChange={(e) => setKey(e.target.value)}
            autoComplete="off"
            spellCheck={false}
            maxLength={512}
          />
          <button type="submit" disabled={busy || !key.trim()}>
            {busy ? "Checking with Google…" : "Save and test"}
          </button>
          <details>
            <summary>How do I get a key?</summary>
            <ol>
              <li>Go to aistudio.google.com and sign in with a Google account.</li>
              <li>
                Choose <strong>Get API key</strong>, then <strong>Create API key</strong>.
              </li>
              <li>Copy the key and paste it into the box above.</li>
            </ol>
          </details>
          <p className="hint-left">
            Without a key I still work, but I only understand a few fixed phrases like{" "}
            <q>open my browser</q>.
          </p>
        </form>
      )}
      {error && <p role="alert">{error}</p>}
      {notice && <p role="status">{notice}</p>}
    </section>
  );
}
