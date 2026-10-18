import { useState } from "react";
import { clearDiagnostics, getDiagnostics } from "../ipc/diagnostics";

/** Help-screen section: lets someone helping the user see what went wrong technically. */
export function Diagnostics() {
  const [text, setText] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  async function show() {
    setNotice(null);
    try {
      setText((await getDiagnostics()) || "Nothing has been recorded yet.");
    } catch {
      setText("The technical details couldn't be loaded.");
    }
  }

  async function copy() {
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      setNotice("Copied. You can paste it into a message to whoever is helping you.");
    } catch {
      setNotice("Couldn't copy automatically. Select the text in the box and copy it.");
    }
  }

  async function clear() {
    await clearDiagnostics().catch(() => {});
    setText(null);
    setNotice("Technical details cleared.");
  }

  return (
    <section className="speech-settings" aria-labelledby="diag-heading">
      <h3 id="diag-heading">Something not working?</h3>
      <p className="hint-left">
        The assistant keeps a short technical record of errors on this computer. It never includes
        what you said, web pages, or any keys, and you can clear it at any time.
      </p>
      <div className="composer-row">
        <button type="button" onClick={() => void show()}>
          Show technical details
        </button>
        <button type="button" onClick={() => void clear()}>
          Clear them
        </button>
      </div>
      {text !== null && (
        <>
          <label htmlFor="diag-text" className="visually-hidden">
            Technical details
          </label>
          <textarea id="diag-text" className="diag-text" readOnly value={text} rows={10} />
          <button type="button" onClick={() => void copy()}>
            Copy technical details
          </button>
        </>
      )}
      {notice && <p role="status">{notice}</p>}
    </section>
  );
}
