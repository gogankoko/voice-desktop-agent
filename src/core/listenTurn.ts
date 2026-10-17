import type { ConversationEvent } from "./conversation/machine";
import type { SpeechRecognizer } from "../voice/types";

export interface ListenDeps {
  recognizer: SpeechRecognizer;
  dispatch: (e: ConversationEvent) => void;
  onInterim: (text: string) => void;
  /** Called with the user's final words before they are acted on. */
  onHeard: (text: string) => void;
  say: (text: string) => void;
  /** False once the user pressed Stop or started another turn. */
  isCurrent: () => boolean;
}

export const NOTHING_HEARD = "I didn't catch that. Press the microphone button and try again.";

/** One listening turn: microphone → final transcript → conversation machine. */
export async function listenTurn(deps: ListenDeps): Promise<void> {
  const { recognizer, dispatch, isCurrent } = deps;
  dispatch({ type: "MIC_START" });

  let text: string;
  try {
    text = await recognizer.listenOnce((t) => isCurrent() && deps.onInterim(t));
  } catch (e) {
    if (!isCurrent()) return;
    const message = e instanceof Error && e.message ? e.message : "Something went wrong while listening.";
    dispatch({ type: "FAILED", error: message });
    deps.say(message);
    dispatch({ type: "RECOVERY_DONE" });
    return;
  }

  if (!isCurrent()) return;
  deps.onInterim("");
  if (!text) {
    // Silence or unclear speech is never treated as a request, or as consent.
    dispatch({ type: "TRANSCRIPT_FINAL", text: "" });
    deps.say(NOTHING_HEARD);
    return;
  }
  deps.onHeard(text);
  dispatch({ type: "TRANSCRIPT_FINAL", text });
}
