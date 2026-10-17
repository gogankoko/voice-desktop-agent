import { type FormEvent, useEffect, useReducer, useRef, useState } from "react";
import { initialState, isActive, transition } from "./core/conversation/machine";
import { type AppStatus, type Availability, getAppStatus } from "./ipc/appStatus";
import { PHASE_TEXT } from "./ui/phaseText";
import "./App.css";

interface Message {
  id: number;
  from: "you" | "assistant";
  text: string;
}

type View = "main" | "help" | "settings";

const NOT_CONNECTED_REPLY =
  "I heard you, but I can't act on requests yet. This early version isn't connected to an assistant or to your computer.";

const AVAILABILITY_TEXT: Record<Availability, string> = {
  not_implemented: "Not available yet",
  mock: "Practice mode (pretend only)",
  available: "Available",
};

export default function App() {
  const [state, dispatch] = useReducer(transition, initialState);
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [draft, setDraft] = useState("");
  const [view, setView] = useState<View>("main");
  const [textScale, setTextScale] = useState(1);
  const nextId = useRef(1);

  useEffect(() => {
    getAppStatus()
      .then(setStatus)
      .catch(() => setStatusError("The app couldn't check which features are available."));
  }, []);

  useEffect(() => {
    document.documentElement.style.setProperty("--text-scale", String(textScale));
  }, [textScale]);

  // Without a reasoning model, every request gets an honest "not yet" reply.
  useEffect(() => {
    if (state.phase !== "understanding") return;
    addMessage("assistant", NOT_CONNECTED_REPLY);
    dispatch({ type: "REPLY_READY" });
  }, [state.phase]);

  // No speech synthesis yet: the reply is shown, so "speaking" completes at once.
  useEffect(() => {
    if (state.phase === "speaking") dispatch({ type: "SPEECH_DONE" });
  }, [state.phase]);

  function addMessage(from: Message["from"], text: string) {
    setMessages((m) => [...m, { id: nextId.current++, from, text }]);
  }

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    const text = draft.trim();
    if (!text) return;
    addMessage("you", text);
    setDraft("");
    dispatch({ type: "TEXT_SUBMITTED", text });
  }

  function onStop() {
    dispatch({ type: "STOP" });
    addMessage("assistant", "Okay, I've stopped.");
  }

  const voiceReady = status?.speechRecognition === "available" || status?.speechRecognition === "mock";
  const phase = PHASE_TEXT[state.phase];

  return (
    <div className="app">
      <header className="topbar">
        <h1>Voice Assistant</h1>
        <nav aria-label="Main">
          <button type="button" aria-current={view === "main" ? "page" : undefined} onClick={() => setView("main")}>
            Home
          </button>
          <button type="button" aria-current={view === "help" ? "page" : undefined} onClick={() => setView("help")}>
            Help
          </button>
          <button
            type="button"
            aria-current={view === "settings" ? "page" : undefined}
            onClick={() => setView("settings")}
          >
            Settings
          </button>
        </nav>
      </header>

      <div className="notice" role="note">
        <strong>Early test version.</strong> This version can't hear you, speak, or control your
        computer yet. You can type, and it will tell you honestly what it can and can't do.
      </div>

      {view === "main" && (
        <main className="main">
          <section className="status" aria-live="polite" aria-atomic="true">
            <span className={`status-symbol phase-${state.phase}`} aria-hidden="true">
              {phase.symbol}
            </span>
            <span className="status-label">{phase.label}</span>
          </section>

          <div className="mic-area">
            <button
              type="button"
              className="mic"
              disabled={!voiceReady}
              aria-describedby="mic-help"
              onClick={() => dispatch({ type: "MIC_START" })}
            >
              <span aria-hidden="true" className="mic-icon">
                🎤
              </span>
              <span>Talk to me</span>
            </button>
            <p id="mic-help" className="hint">
              {voiceReady
                ? "Press to start talking. I will only listen after you press this."
                : "Talking isn't available in this version yet. Please type below."}
            </p>
          </div>

          <section className="transcript" aria-label="Conversation">
            <h2 className="visually-hidden">Conversation</h2>
            {messages.length === 0 ? (
              <p className="hint">Your conversation will appear here.</p>
            ) : (
              <ol aria-live="polite" aria-relevant="additions">
                {messages.map((m) => (
                  <li key={m.id} className={`msg msg-${m.from}`}>
                    <span className="msg-from">{m.from === "you" ? "You" : "Assistant"}:</span>{" "}
                    {m.text}
                  </li>
                ))}
              </ol>
            )}
          </section>

          <form className="composer" onSubmit={onSubmit}>
            <label htmlFor="request">Type a request</label>
            <div className="composer-row">
              <input
                id="request"
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                autoComplete="off"
                maxLength={2000}
              />
              <button type="submit" disabled={!draft.trim()}>
                Send
              </button>
            </div>
          </form>
        </main>
      )}

      {view === "help" && (
        <main className="main panel">
          <h2>Help</h2>
          <p>
            Press <strong>Talk to me</strong> or type what you'd like to do, in your own words.
          </p>
          <p>
            Press <strong>Stop</strong> at any time and I'll stop straight away.
          </p>
          <p>
            Before anything important, like buying something, sending a message, or deleting
            files, I will always tell you exactly what I'm about to do and wait for your OK.
          </p>
        </main>
      )}

      {view === "settings" && (
        <main className="main panel">
          <h2>Settings</h2>
          <label htmlFor="text-size">Text size</label>
          <select
            id="text-size"
            value={textScale}
            onChange={(e) => setTextScale(Number(e.target.value))}
          >
            <option value={1}>Normal</option>
            <option value={1.2}>Large</option>
            <option value={1.4}>Extra large</option>
          </select>

          <h3>What this version can do</h3>
          {statusError && <p role="alert">{statusError}</p>}
          {status && (
            <dl className="capabilities">
              <dt>Hearing you</dt>
              <dd>{AVAILABILITY_TEXT[status.speechRecognition]}</dd>
              <dt>Speaking to you</dt>
              <dd>{AVAILABILITY_TEXT[status.speechSynthesis]}</dd>
              <dt>Understanding requests</dt>
              <dd>{AVAILABILITY_TEXT[status.reasoningModel]}</dd>
              <dt>Controlling your computer</dt>
              <dd>{AVAILABILITY_TEXT[status.computerControl]}</dd>
              <dt>Version</dt>
              <dd>{status.appVersion}</dd>
            </dl>
          )}
        </main>
      )}

      <footer className="stopbar">
        <button
          type="button"
          className="stop"
          onClick={onStop}
          disabled={!isActive(state.phase)}
          aria-keyshortcuts="Escape"
        >
          <span aria-hidden="true">■</span>
          <span>Stop</span>
        </button>
      </footer>
      <StopShortcut onStop={onStop} enabled={isActive(state.phase)} />
    </div>
  );
}

function StopShortcut({ onStop, enabled }: { onStop: () => void; enabled: boolean }) {
  useEffect(() => {
    if (!enabled) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onStop();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onStop, enabled]);
  return null;
}
