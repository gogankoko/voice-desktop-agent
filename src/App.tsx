import { type FormEvent, useEffect, useReducer, useRef, useState } from "react";
import { type Phase, initialState, isActive, transition } from "./core/conversation/machine";
import { type AgentTurnDeps, answerConfirmation, confirmationQuestion, runAgentRequest } from "./core/agentTurn";
import { interpretAnswer } from "./core/consent";
import { listenTurn } from "./core/listenTurn";
import {
  type ActionReport,
  type ConfirmationRequest,
  cancelAgent,
  confirmAgent,
  pauseAgent,
  resumeAgent,
  onAgentExecuting,
  runAgent,
} from "./ipc/agent";
import { type AppStatus, type Availability, getAppStatus } from "./ipc/appStatus";
import { onReminderDue, onTalkShortcut } from "./ipc/events";
import {
  type Preferences,
  TEXT_SCALE,
  type TextSize,
  type Voice,
  getPreferences,
  setPreferences,
  textSizeSchema,
} from "./ipc/preferences";
import { AssistantSettings } from "./ui/AssistantSettings";
import { ConfirmDialog } from "./ui/ConfirmDialog";
import { Reader } from "./ui/Reader";
import { Diagnostics } from "./ui/Diagnostics";
import { SpeechSettings } from "./ui/SpeechSettings";
import { errorText } from "./ui/errorText";
import { MicIcon, WavesIcon } from "./ui/icons";
import { PHASE_TEXT } from "./ui/phaseText";
import { playChime } from "./voice/chime";
import type { SpeechRecognizer } from "./voice/types";
import { ReplyVoice, WebAudioPlayer } from "./voice/replyVoice";
import { WindowsSynthesizer } from "./voice/windowsSynthesizer";
import { speakNatural } from "./ipc/speech";
import { logUiError } from "./ipc/diagnostics";
import "./App.css";

interface Message {
  id: number;
  /** "action": what the app itself observed happening on the computer (not spoken). */
  from: "you" | "assistant" | "action";
  text: string;
  ok?: boolean;
}

const SPEAKER: Record<Message["from"], string> = { you: "You", assistant: "Assistant", action: "Action" };

type View = "main" | "help" | "settings";

const VOICE_LABEL: Record<Voice, string> = {
  ava: "Ava (US)",
  andrew: "Andrew (US)",
  emma: "Emma (US)",
  brian: "Brian (US)",
  sonia: "Sonia (UK)",
  ryan: "Ryan (UK)",
  windows: "Windows voice (offline)",
};

const VOICES: Voice[] = ["ava", "andrew", "emma", "brian", "sonia", "ryan", "windows"];

const TEXT_SIZE_LABEL: Record<TextSize, string> = {
  normal: "Normal",
  large: "Large",
  extra_large: "Extra large",
};

const AVAILABILITY_TEXT: Record<Availability, string> = {
  not_implemented: "Not available yet",
  needs_setup: "Needs setting up (see below)",
  mock: "Practice mode (pretend only)",
  limited: "Your web browser, making the screen bigger, and finding your files (if allowed)",
  available: "Available",
};

/** The assistant is working on a request (Pause is offered). */
const WORKING: ReadonlySet<Phase> = new Set(["understanding", "planning", "executing", "verifying"]);

/** Phases in which pressing the microphone starts a new turn (barge-in while speaking). */
const MIC_STARTS: ReadonlySet<Phase> = new Set(["idle", "cancelled", "speaking"]);

export default function App() {
  const [state, dispatch] = useReducer(transition, initialState);
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [draft, setDraft] = useState("");
  const [interim, setInterim] = useState("");
  const [view, setView] = useState<View>("main");
  const [prefs, setPrefs] = useState<Preferences>({
    textSize: "normal",
    speakReplies: true,
    practiceMode: false,
    fileSearch: false,
    voice: "ava",
  });
  const [question, setQuestion] = useState<ConfirmationRequest | null>(null);
  const [answerHint, setAnswerHint] = useState<string | null>(null);
  const [answerListening, setAnswerListening] = useState(false);
  const [stepText, setStepText] = useState("");
  const [reading, setReading] = useState<{ title: string; text: string; key: number } | null>(null);
  const [prefsError, setPrefsError] = useState<string | null>(null);
  // Replies: the chosen natural voice (Azure), falling back to the Windows voice. Long
  // read-aloud articles always use the Windows voice, so they don't use up the free allowance.
  const [windowsVoice] = useState(() => new WindowsSynthesizer());
  const [synth] = useState(
    () =>
      new ReplyVoice(windowsVoice, speakNatural, new WebAudioPlayer(), Date.now, (why) =>
        logUiError(`natural voice failed, using the Windows voice: ${why}`),
      ),
  );
  const [voiceInstalled, setVoiceInstalled] = useState(windowsVoice.available);
  const nextId = useRef(1);
  // Incremented by Stop and by each new turn; work whose id is stale must not act or report.
  const taskId = useRef(0);
  const recognizer = useRef<SpeechRecognizer | null>(null);
  const talkShortcut = useRef<() => void>(() => {});
  const reminderRef = useRef<(text: string) => void>(() => {});
  const speakReplies = useRef(prefs.speakReplies);
  speakReplies.current = prefs.speakReplies;
  const assistantReady = useRef(false);
  assistantReady.current = status?.reasoningModel === "available";

  function refreshStatus() {
    getAppStatus()
      .then(setStatus)
      .catch(() => setStatusError("The app couldn't check which features are available."));
  }

  useEffect(() => {
    refreshStatus();
    getPreferences()
      .then(setPrefs)
      .catch((e) => setPrefsError(errorText(e, "Your settings couldn't be loaded.")));
    const onVoices = () => setVoiceInstalled(windowsVoice.available);
    globalThis.speechSynthesis?.addEventListener("voiceschanged", onVoices);
    return () => globalThis.speechSynthesis?.removeEventListener("voiceschanged", onVoices);
  }, [windowsVoice]);

  useEffect(() => {
    synth.setVoice(prefs.voice, status?.speechRecognition === "available");
  }, [synth, prefs.voice, status?.speechRecognition]);

  useEffect(() => {
    document.documentElement.style.setProperty("--text-scale", String(TEXT_SCALE[prefs.textSize]));
  }, [prefs.textSize]);

  function changePrefs(next: Preferences) {
    const previous = prefs;
    setPrefs(next);
    setPrefsError(null);
    if (!next.speakReplies) synth.cancel();
    setPreferences(next).catch((e) => {
      setPrefs(previous);
      setPrefsError(errorText(e, "Your settings couldn't be saved."));
    });
  }

  function addMessage(from: Message["from"], text: string) {
    setMessages((m) => [...m, { id: nextId.current++, from, text }]);
  }

  function noteAction(a: ActionReport) {
    setMessages((m) => [...m, { id: nextId.current++, from: "action", text: a.summary, ok: a.ok && a.verified }]);
  }

  function say(text: string) {
    addMessage("assistant", text);
    if (speakReplies.current) synth.speak(text);
  }

  useEffect(() => {
    if (state.phase !== "understanding" || state.utterance === null) return;
    // Always to the backend: built-in commands work without a Gemini key; anything else
    // needs one, and the backend says so.
    const id = ++taskId.current;
    void runAgentRequest(state.utterance, agentDeps(id));
  }, [state.phase, state.utterance]);

  function agentDeps(id: number): AgentTurnDeps {
    return {
      runAgent: (t) => runAgent(id, t),
      confirmAgent: (cid, approved) => confirmAgent(id, cid, approved),
      onExecuting: (cb) => onAgentExecuting(id, cb),
      onProgress: setStepText,
      readAloud: (r) => setReading({ ...r, key: Date.now() }),
      dispatch,
      say,
      note: noteAction,
      askConfirmation: (c) => {
        setAnswerHint(null);
        setQuestion(c);
        say(confirmationQuestion(c));
      },
      isCurrent: () => taskId.current === id,
    };
  }

  /** The only path from the confirmation screen to the backend: an explicit yes or no. */
  function answer(approved: boolean, how: string) {
    const q = question;
    if (!q) return;
    setQuestion(null);
    setAnswerHint(null);
    synth.cancel();
    recognizer.current?.cancel();
    addMessage("you", how);
    void answerConfirmation(q, approved, agentDeps(taskId.current));
  }

  async function answerBySpeaking() {
    const q = question;
    if (!q) return;
    synth.cancel();
    setAnswerHint(null);
    if (!recognizer.current) {
      const { AzureRecognizer } = await import("./voice/azureRecognizer");
      recognizer.current = new AzureRecognizer();
    }
    setAnswerListening(true);
    let heard = "";
    try {
      heard = await recognizer.current.listenOnce(() => {});
    } catch (e) {
      setAnswerHint(errorText(e, "I couldn't listen just now. Please press a button."));
      return;
    } finally {
      setAnswerListening(false);
    }
    const verdict = interpretAnswer(heard);
    if (verdict === "yes") answer(true, `Yes (said: "${heard}")`);
    else if (verdict === "no") answer(false, `No (said: "${heard}")`);
    else
      setAnswerHint(
        heard
          ? `I heard "${heard}". To be safe, please say just "yes" or "no", or press a button.`
          : "I didn't hear anything. Please say yes or no, or press a button.",
      );
  }

  // An unanswered question is never consent: when it expires, it's a no.
  useEffect(() => {
    if (!question) return;
    const t = setTimeout(() => answer(false, "No answer in time"), question.expiresInSecs * 1000 - 2000);
    return () => clearTimeout(t);
  }, [question]);

  // "Speaking" ends when the voice has finished (immediately if replies aren't read aloud).
  useEffect(() => {
    if (state.phase !== "speaking") return;
    let live = true;
    void synth.whenIdle().then(() => live && dispatch({ type: "SPEECH_DONE" }));
    return () => {
      live = false;
    };
  }, [state.phase, synth]);

  async function onMic() {
    if (state.phase === "listening") {
      onStop();
      return;
    }
    if (!MIC_STARTS.has(state.phase)) return;
    synth.cancel(); // barge-in: stop talking as soon as the user wants to speak
    setReading(null);
    const id = ++taskId.current;
    if (!recognizer.current) {
      // Loaded on first use so the large speech SDK doesn't slow down startup.
      const { AzureRecognizer } = await import("./voice/azureRecognizer");
      recognizer.current = new AzureRecognizer();
    }
    if (taskId.current !== id) return;
    void listenTurn({
      recognizer: recognizer.current,
      dispatch,
      onInterim: setInterim,
      onHeard: (text) => addMessage("you", text),
      say,
      isCurrent: () => taskId.current === id,
    });
  }

  function remind(text: string) {
    playChime();
    const line = `Reminder: ${text}`;
    addMessage("assistant", `⏰ ${line}`);
    if (speakReplies.current) synth.speak(line);
  }

  useEffect(() => {
    const subscriptions = [
      onTalkShortcut(() => talkShortcut.current()),
      onReminderDue((text) => reminderRef.current(text)),
    ];
    return () => {
      for (const s of subscriptions) void s.then((unsubscribe) => unsubscribe()).catch(() => {});
    };
  }, []);

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    const text = draft.trim();
    if (!text) return;
    synth.cancel();
    setReading(null);
    addMessage("you", text);
    setDraft("");
    dispatch({ type: "TEXT_SUBMITTED", text });
  }

  async function onPause() {
    if (await pauseAgent().catch(() => false)) {
      dispatch({ type: "PAUSE" });
      setStepText("");
    }
  }

  async function onResume() {
    await resumeAgent().catch(() => false);
    dispatch({ type: "RESUME" });
  }

  function onStop() {
    taskId.current++;
    setQuestion(null);
    setStepText("");
    setReading(null);
    void cancelAgent().catch(() => {});
    recognizer.current?.cancel();
    synth.cancel();
    setInterim("");
    dispatch({ type: "STOP" });
    addMessage("assistant", "Okay, I've stopped.");
  }

  const listening = state.phase === "listening";
  const voiceReady = status?.speechRecognition === "available";
  const micEnabled = voiceReady && (listening || MIC_STARTS.has(state.phase));
  reminderRef.current = remind;
  // The talk shortcut does what the visible button would: answer an open question, or talk.
  talkShortcut.current = () => {
    setView("main");
    if (question) void answerBySpeaking();
    else if (micEnabled) void onMic();
  };
  const phase = PHASE_TEXT[state.phase];

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <span className="brand-logo" aria-hidden="true">
            <MicIcon size="1.2em" />
          </span>
          <h1>Voice Assistant</h1>
        </div>
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
        <strong>Early test version.</strong> You can talk to me in your own words. I can work in your
        web browser and your apps (like Spotify), set reminders, make your screen bigger, and find
        your files if you allow it in Settings.
      </div>
      {prefs.practiceMode && (
        <div className="practice-banner" role="note">
          <span aria-hidden="true">🎓 </span>Practice mode is on: you can try important actions like
          sending a message. Nothing is really sent.
        </div>
      )}

      {view === "main" && (
        <main className="main">
          <section className="status" aria-live="polite" aria-atomic="true">
            <span className={`status-symbol phase-${state.phase}`} aria-hidden="true">
              {phase.symbol}
            </span>
            <span className="status-label">
              {stepText && (state.phase === "executing" || state.phase === "planning")
                ? `${stepText}…`
                : phase.label}
            </span>
          </section>

          <div className="mic-area">
            <button
              type="button"
              className={`mic${listening ? " mic-listening" : ""}`}
              disabled={!micEnabled}
              aria-pressed={listening}
              aria-describedby="mic-help"
              onClick={() => void onMic()}
            >
              <span aria-hidden="true" className="mic-icon">
                {listening ? <WavesIcon /> : <MicIcon />}
              </span>
              <span>{listening ? "Listening… press to stop" : "Talk to me"}</span>
            </button>
            <p id="mic-help" className="hint">
              {!status
                ? ""
                : !voiceReady
                  ? "To talk to me, set up Listening in Settings. You can type below meanwhile."
                  : listening
                    ? "The microphone is on. Speak now, then pause when you're done."
                    : `Press to talk${status.talkShortcut ? `, or press ${status.talkShortcut} from anywhere` : ""}. The microphone is only on while this button says Listening.`}
            </p>
            {!voiceReady && status && (
              <button type="button" onClick={() => setView("settings")}>
                Set up listening
              </button>
            )}
            {listening && (
              <p className="interim" aria-live="off">
                {interim ? <>Hearing: <q>{interim}</q></> : "Waiting for you to speak…"}
              </p>
            )}
          </div>

          {reading && (
            <Reader
              key={reading.key}
              title={reading.title}
              text={reading.text}
              synth={windowsVoice}
              onClose={() => setReading(null)}
            />
          )}

          <section className="transcript" aria-label="Conversation">
            <h2 className="visually-hidden">Conversation</h2>
            {messages.length === 0 ? (
              <p className="hint">Your conversation will appear here.</p>
            ) : (
              <ol aria-live="polite" aria-relevant="additions">
                {messages.map((m) => (
                  <li key={m.id} className={`msg msg-${m.from}${m.from === "action" && !m.ok ? " msg-warn" : ""}`}>
                    <span className="msg-from">{SPEAKER[m.from]}:</span>{" "}
                    {m.from === "action" && <span aria-hidden="true">{m.ok ? "✓ " : "⚠ "}</span>}
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
            Press <strong>Talk to me</strong> and say what you'd like, in your own words. Or type it
            instead.
          </p>
          {status?.talkShortcut ? (
            <p>
              From any program, press <strong>{status.talkShortcut}</strong> to bring me forward and
              start talking. It also answers my yes-or-no questions by voice.
            </p>
          ) : (
            <p>
              The keyboard shortcut for talking isn't available, because other programs already use
              all of the ones I can use.
            </p>
          )}
          <p>
            Things you can ask: "play Yeat on Spotify", "pause the music", "turn it up", "remind me
            in 20 minutes to take my pills", "switch to Notepad", "how much battery do I have?".
            Reminders only work while this app is open.
          </p>
          <p>
            Simple commands work instantly, even without the AI: "open YouTube", "open Spotify",
            "pause", "next song", "turn it up", "set a timer for 10 minutes", "what time is it?".
          </p>
          <p>
            The microphone is only on while the button says <strong>Listening</strong>. Press it
            again, or press <strong>Stop</strong>, to turn it off.
          </p>
          <p>
            Press <strong>Stop</strong> (or the Esc key) at any time and I'll stop straight away,
            including talking.
          </p>
          <p>
            Before anything important, like buying something, sending a message, or deleting
            files, I will always tell you exactly what I'm about to do and wait for your OK.
          </p>
          <Diagnostics />
        </main>
      )}

      {view === "settings" && (
        <main className="main panel">
          <h2>Settings</h2>
          <section className="card" aria-labelledby="display-heading">
          <h3 id="display-heading">Display and voice</h3>
          <fieldset className="choices">
            <legend>Text size</legend>
            {textSizeSchema.options.map((size) => (
              <label key={size} className="choice">
                <input
                  type="radio"
                  name="text-size"
                  value={size}
                  checked={prefs.textSize === size}
                  onChange={() => changePrefs({ ...prefs, textSize: size })}
                />
                {TEXT_SIZE_LABEL[size]}
              </label>
            ))}
          </fieldset>

          <label className="choice toggle">
            <input
              type="checkbox"
              checked={prefs.speakReplies}
              onChange={(e) => changePrefs({ ...prefs, speakReplies: e.target.checked })}
            />
            Read my replies aloud
          </label>
          <label className="choice toggle">
            <input
              type="checkbox"
              checked={prefs.practiceMode}
              onChange={(e) => changePrefs({ ...prefs, practiceMode: e.target.checked })}
            />
            Practice mode (try sending a pretend message; nothing is really sent)
          </label>
          <fieldset className="choices">
            <legend>Voice for replies</legend>
            {VOICES.map((v) => (
              <label key={v} className="choice">
                <input
                  type="radio"
                  name="voice"
                  value={v}
                  checked={prefs.voice === v}
                  onChange={() => changePrefs({ ...prefs, voice: v })}
                />
                {VOICE_LABEL[v]}
              </label>
            ))}
            <button
              type="button"
              className="try-voice"
              onClick={() => synth.preview(`Hello, I'm ${VOICE_LABEL[prefs.voice].split(" (")[0]}. This is how I'll sound.`, prefs.voice)}
            >
              <span aria-hidden="true">▶ </span>Try this voice
            </button>
          </fieldset>
          <p className="hint-left">
            {status?.speechRecognition === "available"
              ? "Natural voices are made by Microsoft Azure: the text of each reply is sent to Microsoft. Long articles are always read with the Windows voice."
              : "Natural voices need the Azure Speech key (set up Listening below). Until then, the Windows voice is used."}
          </p>
          </section>

          <section className="speech-settings" aria-labelledby="files-heading">
            <h3 id="files-heading">Finding your files</h3>
            <p className="hint-left">
              When this is on, I can look for files by their <strong>name</strong> in your Downloads,
              Pictures, Documents and Desktop folders, and open photos, documents, videos and music
              or show a file in its folder. I never read what's inside files, and I never change,
              move or delete anything. File names I find are sent to Google so I can understand them.
            </p>
            <label className="choice toggle">
              <input
                type="checkbox"
                checked={prefs.fileSearch}
                onChange={(e) => changePrefs({ ...prefs, fileSearch: e.target.checked })}
              />
              Let the assistant search my files
            </label>
          </section>
          {prefsError && <p role="alert">{prefsError}</p>}

          <SpeechSettings onChanged={refreshStatus} />
          <AssistantSettings onChanged={refreshStatus} />

          <h3>What this version can do</h3>
          {statusError && <p role="alert">{statusError}</p>}
          {status && (
            <dl className="capabilities">
              <dt>Hearing you</dt>
              <dd>{AVAILABILITY_TEXT[status.speechRecognition]}</dd>
              <dt>Speaking to you</dt>
              <dd>
                {prefs.voice !== "windows" && status.speechRecognition === "available"
                  ? `Natural voice: ${VOICE_LABEL[prefs.voice]} (Azure), with the Windows voice as a backup`
                  : voiceInstalled
                    ? "Available (Windows voice)"
                    : "No Windows voice found"}
              </dd>
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

      {question && (
        <ConfirmDialog
          request={question}
          onAnswer={(approved) => answer(approved, approved ? "Yes (pressed Yes)" : "No (pressed No)")}
          onAnswerBySpeaking={status?.speechRecognition === "available" ? () => void answerBySpeaking() : undefined}
          listening={answerListening}
          hint={answerHint}
        />
      )}

      <footer className="stopbar">
        {assistantReady.current && (WORKING.has(state.phase) || state.phase === "paused") && (
          <button
            type="button"
            className="pause"
            onClick={() => void (state.phase === "paused" ? onResume() : onPause())}
          >
            {state.phase === "paused" ? (
              <>
                <span aria-hidden="true">▶</span>
                <span>Continue</span>
              </>
            ) : (
              <>
                <span aria-hidden="true">❚❚</span>
                <span>Pause</span>
              </>
            )}
          </button>
        )}
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
