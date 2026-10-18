import type { ConversationEvent } from "./conversation/machine";
import type { ActionReport, AgentOutcome, ConfirmationRequest } from "../ipc/agent";

export interface AgentTurnDeps {
  runAgent: (text: string) => Promise<AgentOutcome>;
  confirmAgent: (confirmationId: string, approved: boolean) => Promise<AgentOutcome>;
  /** Subscribes to "a tool started" for this task; returns an unsubscribe. */
  onExecuting: (cb: () => void) => Promise<() => void>;
  dispatch: (e: ConversationEvent) => void;
  say: (text: string) => void;
  /** Records an action as observed by the app (shown, not spoken). */
  note: (action: ActionReport) => void;
  /** Shows the confirmation screen. The answer comes back through `answerConfirmation`. */
  askConfirmation: (request: ConfirmationRequest) => void;
  /** False once the user pressed Stop or started a newer request. */
  isCurrent: () => boolean;
}

const DECLINED_REPLY = "Okay, I won't do that.";

/** Words spoken when the confirmation screen opens. Built from the same validated details it shows. */
export function confirmationQuestion(c: ConfirmationRequest): string {
  const details = c.details.map((d) => `${d.label}: ${d.value}.`).join(" ");
  return `Before I go ahead, please check. ${c.action}. ${details} Shall I do it? Say yes or no, or press a button.`;
}

/**
 * One request handled by the reasoning model. The model's words are spoken, but what
 * actually happened is taken from the backend's action reports, never from the model.
 */
export async function runAgentRequest(text: string, deps: AgentTurnDeps): Promise<void> {
  await withProgress(deps, (startExecuting) =>
    call(deps, () => deps.runAgent(text), startExecuting),
  );
}

/**
 * The user's explicit answer from the confirmation screen. Approval moves straight to
 * executing; a "no" goes back to planning while the assistant acknowledges it.
 */
export async function answerConfirmation(
  request: ConfirmationRequest,
  approved: boolean,
  deps: AgentTurnDeps,
): Promise<void> {
  deps.dispatch(
    approved ? { type: "CONFIRMED", confirmationId: request.id } : { type: "DECLINED" },
  );
  await withProgress(deps, (startExecuting) =>
    call(deps, () => deps.confirmAgent(request.id, approved), startExecuting, !approved),
  );
}

type StartExecuting = () => void;

async function withProgress(deps: AgentTurnDeps, body: (s: StartExecuting) => Promise<void>) {
  let executing = false;
  const startExecuting = () => {
    if (executing || !deps.isCurrent()) return;
    executing = true;
    deps.dispatch({ type: "INTENT_READY" }); // no-ops when already past understanding
    deps.dispatch({ type: "PLAN_READY" });
  };
  const unsubscribe = await deps.onExecuting(startExecuting);
  try {
    await body(startExecuting);
  } finally {
    unsubscribe();
  }
}

async function call(
  deps: AgentTurnDeps,
  request: () => Promise<AgentOutcome>,
  startExecuting: StartExecuting,
  declined = false,
): Promise<void> {
  const { dispatch, isCurrent } = deps;
  let outcome: AgentOutcome;
  try {
    outcome = await request();
  } catch (e) {
    if (!isCurrent()) return;
    const message = typeof e === "string" && e ? e : "Something went wrong. Please try again.";
    dispatch({ type: "FAILED", error: message });
    deps.say(declined ? `${DECLINED_REPLY} ${message}` : message);
    dispatch({ type: "RECOVERY_DONE" });
    return;
  }

  if (!isCurrent() || outcome.cancelled) {
    const happened = outcome.actions.filter((a) => a.ok && !a.practice);
    outcome.actions.filter((a) => a.ok).forEach(deps.note);
    if (happened.length > 0) {
      deps.say("You asked me to stop, but something had already started. You can close it if you like.");
    }
    return;
  }

  outcome.actions.forEach(deps.note);

  if (outcome.confirmation) {
    if (outcome.actions.length > 0) startExecuting();
    dispatch({ type: "INTENT_READY" });
    dispatch({ type: "NEEDS_CONFIRMATION", confirmationId: outcome.confirmation.id });
    deps.askConfirmation(outcome.confirmation);
    return;
  }

  const reply = outcome.reply || (declined ? DECLINED_REPLY : "");
  if (outcome.actions.length === 0) {
    dispatch({ type: "REPLY_READY" });
    deps.say(reply);
    return;
  }
  startExecuting();
  dispatch({ type: "ACTION_DONE" });
  if (outcome.actions.every((a) => a.ok && a.verified)) {
    dispatch({ type: "VERIFIED" });
    deps.say(reply);
  } else {
    dispatch({ type: "FAILED", error: "An action couldn't be confirmed." });
    deps.say(reply);
    dispatch({ type: "RECOVERY_DONE" });
  }
}
