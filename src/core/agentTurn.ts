import type { ConversationEvent } from "./conversation/machine";
import type { ActionReport, AgentOutcome } from "../ipc/agent";

export interface AgentTurnDeps {
  runAgent: (text: string) => Promise<AgentOutcome>;
  /** Subscribes to "a tool started" for this task; returns an unsubscribe. */
  onExecuting: (cb: () => void) => Promise<() => void>;
  dispatch: (e: ConversationEvent) => void;
  say: (text: string) => void;
  /** Records an action as observed by the app (shown, not spoken). */
  note: (action: ActionReport) => void;
  /** False once the user pressed Stop or started a newer request. */
  isCurrent: () => boolean;
}

/**
 * One request handled by the reasoning model. The model's words are spoken, but what
 * actually happened is taken from the backend's action reports, never from the model.
 */
export async function runAgentRequest(text: string, deps: AgentTurnDeps): Promise<void> {
  const { dispatch, isCurrent } = deps;
  let executing = false;
  const startExecuting = () => {
    if (executing || !isCurrent()) return;
    executing = true;
    dispatch({ type: "INTENT_READY" });
    dispatch({ type: "PLAN_READY" }); // only low-risk tools exist; no confirmation step yet
  };
  const unsubscribe = await deps.onExecuting(startExecuting);

  try {
    let outcome: AgentOutcome;
    try {
      outcome = await deps.runAgent(text);
    } catch (e) {
      if (!isCurrent()) return;
      const message = typeof e === "string" && e ? e : "Something went wrong. Please try again.";
      dispatch({ type: "FAILED", error: message });
      deps.say(message);
      dispatch({ type: "RECOVERY_DONE" });
      return;
    }

    if (!isCurrent() || outcome.cancelled) {
      const happened = outcome.actions.filter((a) => a.ok);
      happened.forEach(deps.note);
      if (happened.length > 0) {
        deps.say("You asked me to stop, but something had already started. You can close it if you like.");
      }
      return;
    }

    outcome.actions.forEach(deps.note);
    if (outcome.actions.length === 0) {
      dispatch({ type: "REPLY_READY" });
      deps.say(outcome.reply);
      return;
    }
    startExecuting();
    dispatch({ type: "ACTION_DONE" });
    if (outcome.actions.every((a) => a.ok && a.verified)) {
      dispatch({ type: "VERIFIED" });
      deps.say(outcome.reply);
    } else {
      dispatch({ type: "FAILED", error: "An action couldn't be confirmed." });
      deps.say(outcome.reply);
      dispatch({ type: "RECOVERY_DONE" });
    }
  } finally {
    unsubscribe();
  }
}
