import type { ConversationEvent } from "./conversation/machine";
import type { Intent } from "./intents";
import type { OpenResult } from "../ipc/tools";

export interface TaskDeps {
  dispatch: (e: ConversationEvent) => void;
  say: (text: string) => void;
  openApplication: (app: "default_browser") => Promise<OpenResult>;
  /** False once the user pressed Stop or started a newer request. */
  isCurrent: () => boolean;
}

export const UNRECOGNISED_REPLY =
  "Sorry, I can only do a few things so far. Try saying “open my browser”.";

/**
 * Runs one request through planning, execution and verification.
 * Never reports success unless the backend verified the outcome.
 */
export async function runRequest(intent: Intent | null, deps: TaskDeps): Promise<void> {
  const { dispatch, say, isCurrent } = deps;

  if (!intent) {
    say(UNRECOGNISED_REPLY);
    dispatch({ type: "REPLY_READY" });
    return;
  }

  dispatch({ type: "INTENT_READY" });
  say("Opening your web browser.");
  dispatch({ type: "PLAN_READY" }); // low risk: no confirmation needed

  let result: OpenResult;
  try {
    result = await deps.openApplication(intent.app);
  } catch (e) {
    if (!isCurrent()) return;
    const message = typeof e === "string" && e ? e : "Something went wrong while opening it.";
    dispatch({ type: "FAILED", error: message });
    say(`${message} Nothing else was changed.`);
    dispatch({ type: "RECOVERY_DONE" });
    return;
  }

  if (!isCurrent()) {
    // The launch can't be taken back once Windows has started it.
    say("You asked me to stop, but the browser may already have opened. You can close it if you like.");
    return;
  }

  dispatch({ type: "ACTION_DONE" });
  if (result.verified) {
    dispatch({ type: "VERIFIED" });
    say(`${result.appName} is open and in front of you now.`);
  } else {
    dispatch({ type: "FAILED", error: "Couldn't confirm the browser opened." });
    say(`I asked Windows to open ${result.appName}, but I couldn't see it appear. Please check your screen.`);
    dispatch({ type: "RECOVERY_DONE" });
  }
}
