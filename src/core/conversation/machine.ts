/**
 * Conversation state machine. Pure and synchronous so every transition is unit-testable.
 *
 * Safety rules enforced here (not in prompts):
 * - Only an explicit CONFIRMED event carrying the matching confirmation id leaves
 *   `awaiting_confirmation` for `executing`. Speech, silence, and stale or duplicate
 *   confirmations cannot.
 * - STOP is accepted from every active phase and lands in `cancelled`.
 * - Events that are not valid for the current phase are ignored, so duplicate or
 *   late events (network retries, repeated speech results) cannot advance a task twice.
 */

export const PHASES = [
  "idle",
  "listening",
  "transcribing",
  "understanding",
  "planning",
  "awaiting_confirmation",
  "executing",
  "verifying",
  "speaking",
  "paused",
  "recovering",
  "cancelled",
] as const;

export type Phase = (typeof PHASES)[number];

export interface ConversationState {
  phase: Phase;
  /** The user's latest request, as heard or typed. */
  utterance: string | null;
  /** Set only while awaiting confirmation; a CONFIRMED event must echo it. */
  pendingConfirmationId: string | null;
  /** Phase to return to after RESUME. */
  resumePhase: Phase | null;
  /** Plain-language description of the last failure, if any. */
  error: string | null;
}

export type ConversationEvent =
  | { type: "MIC_START" }
  | { type: "AUDIO_END" }
  | { type: "TRANSCRIPT_FINAL"; text: string }
  | { type: "TEXT_SUBMITTED"; text: string }
  | { type: "INTENT_READY" }
  | { type: "REPLY_READY" }
  | { type: "PLAN_READY"; confirmationId?: string }
  | { type: "NEEDS_CONFIRMATION"; confirmationId: string }
  | { type: "CONFIRMED"; confirmationId: string }
  | { type: "DECLINED" }
  | { type: "ACTION_DONE" }
  | { type: "VERIFIED" }
  | { type: "FAILED"; error: string }
  | { type: "RECOVERY_DONE" }
  | { type: "SPEECH_DONE" }
  | { type: "PAUSE" }
  | { type: "RESUME" }
  | { type: "STOP" }
  | { type: "RESET" };

export const initialState: ConversationState = {
  phase: "idle",
  utterance: null,
  pendingConfirmationId: null,
  resumePhase: null,
  error: null,
};

/** Phases where the user may start a new request (barge-in allowed while speaking). */
const READY_FOR_INPUT: ReadonlySet<Phase> = new Set(["idle", "cancelled", "speaking"]);
const CAN_FAIL: ReadonlySet<Phase> = new Set([
  "listening",
  "transcribing",
  "understanding",
  "planning",
  "executing",
  "verifying",
]);
const CAN_PAUSE: ReadonlySet<Phase> = new Set(["executing", "speaking"]);
const ASKS_CONFIRMATION: ReadonlySet<Phase> = new Set(["understanding", "planning", "executing"]);

function to(state: ConversationState, phase: Phase, patch: Partial<ConversationState> = {}) {
  return { ...state, phase, pendingConfirmationId: null, resumePhase: null, ...patch };
}

function startRequest(state: ConversationState, text: string): ConversationState {
  const trimmed = text.trim();
  if (!trimmed) return to(state, "idle", { utterance: null });
  return to(state, "understanding", { utterance: trimmed, error: null });
}

export function transition(state: ConversationState, event: ConversationEvent): ConversationState {
  const { phase } = state;

  switch (event.type) {
    case "STOP":
      return phase === "idle" ? state : to(state, "cancelled");

    case "RESET":
      return phase === "cancelled" ? to(initialState, "idle") : state;

    case "MIC_START":
      return READY_FOR_INPUT.has(phase) ? to(state, "listening", { error: null }) : state;

    case "AUDIO_END":
      return phase === "listening" ? to(state, "transcribing") : state;

    case "TRANSCRIPT_FINAL":
      return phase === "listening" || phase === "transcribing"
        ? startRequest(state, event.text)
        : state;

    case "TEXT_SUBMITTED":
      return READY_FOR_INPUT.has(phase) ? startRequest(state, event.text) : state;

    case "INTENT_READY":
      return phase === "understanding" ? to(state, "planning") : state;

    case "REPLY_READY":
      // From executing: an approved step turned out to have nothing to do (e.g. it expired).
      return phase === "understanding" || phase === "planning" || phase === "executing"
        ? to(state, "speaking")
        : state;

    case "PLAN_READY":
      if (phase !== "planning") return state;
      return event.confirmationId
        ? to(state, "awaiting_confirmation", { pendingConfirmationId: event.confirmationId })
        : to(state, "executing");

    case "NEEDS_CONFIRMATION":
      // The backend paused before a high-risk action (possibly after earlier steps ran).
      return ASKS_CONFIRMATION.has(phase) && event.confirmationId
        ? to(state, "awaiting_confirmation", { pendingConfirmationId: event.confirmationId })
        : state;

    case "CONFIRMED":
      return phase === "awaiting_confirmation" &&
        state.pendingConfirmationId !== null &&
        event.confirmationId === state.pendingConfirmationId
        ? to(state, "executing")
        : state;

    case "DECLINED":
      // Back to planning: the assistant works out what to say (or do) instead.
      return phase === "awaiting_confirmation" ? to(state, "planning") : state;

    case "ACTION_DONE":
      return phase === "executing" ? to(state, "verifying") : state;

    case "VERIFIED":
      return phase === "verifying" ? to(state, "speaking") : state;

    case "FAILED":
      return CAN_FAIL.has(phase) ? to(state, "recovering", { error: event.error }) : state;

    case "RECOVERY_DONE":
      return phase === "recovering" ? to(state, "speaking") : state;

    case "SPEECH_DONE":
      return phase === "speaking" ? to(state, "idle") : state;

    case "PAUSE":
      return CAN_PAUSE.has(phase)
        ? { ...state, phase: "paused", resumePhase: phase }
        : state;

    case "RESUME":
      return phase === "paused" && state.resumePhase
        ? { ...state, phase: state.resumePhase, resumePhase: null }
        : state;
  }
}

/** True while the assistant is working on something the Stop button should end. */
export function isActive(phase: Phase): boolean {
  return phase !== "idle" && phase !== "cancelled";
}
