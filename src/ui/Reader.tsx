import { useEffect, useMemo, useState } from "react";
import { splitIntoChunks } from "../core/chunks";
import type { SpeechSynthesizer } from "../voice/types";

interface Props {
  title: string;
  text: string;
  synth: SpeechSynthesizer;
  onClose: () => void;
}

/** Reads a long text aloud part by part, with Pause / Continue / Stop reading. */
export function Reader({ title, text, synth, onClose }: Props) {
  const parts = useMemo(() => splitIntoChunks(text), [text]);
  const [index, setIndex] = useState(0);
  const [paused, setPaused] = useState(false);
  const finished = index >= parts.length;

  useEffect(() => {
    if (paused || finished) return;
    let live = true;
    synth.speak(parts[index]);
    void synth.whenIdle().then(() => {
      if (live) setIndex((i) => i + 1);
    });
    return () => {
      live = false;
    };
  }, [index, paused, finished, parts, synth]);

  // Silence when the reader goes away (Stop, a new request, the microphone).
  useEffect(() => () => synth.cancel(), [synth]);

  function pause() {
    setPaused(true);
    synth.cancel();
  }

  return (
    <section className="reader" aria-labelledby="reader-title">
      <h2 id="reader-title">
        <span aria-hidden="true">📖 </span>
        {finished ? "Finished reading" : paused ? "Paused" : "Reading"}: {title || "this page"}
      </h2>
      {!finished && <p className="reader-text">{parts[index]}</p>}
      <p className="hint-left">
        Part {Math.min(index + 1, parts.length)} of {parts.length}
      </p>
      <div className="composer-row">
        {!finished &&
          (paused ? (
            <button type="button" onClick={() => setPaused(false)}>
              <span aria-hidden="true">▶ </span>Continue
            </button>
          ) : (
            <button type="button" onClick={pause}>
              <span aria-hidden="true">❚❚ </span>Pause
            </button>
          ))}
        {paused && index > 0 && (
          <button type="button" onClick={() => setIndex((i) => i - 1)}>
            Go back a bit
          </button>
        )}
        <button type="button" onClick={onClose}>
          {finished ? "Close" : "Stop reading"}
        </button>
      </div>
    </section>
  );
}
