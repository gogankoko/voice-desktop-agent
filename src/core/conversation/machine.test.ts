import { describe, expect, it } from "vitest";
import {
  type ConversationEvent,
  type ConversationState,
  PHASES,
  initialState,
  isActive,
  transition,
} from "./machine";

function run(events: ConversationEvent[], start: ConversationState = initialState) {
  return events.reduce(transition, start);
}

const toConfirmation: ConversationEvent[] = [
  { type: "TEXT_SUBMITTED", text: "buy the milk" },
  { type: "INTENT_READY" },
  { type: "PLAN_READY", confirmationId: "c-1" },
];

describe("conversation machine", () => {
  it("does not start listening on its own", () => {
    expect(initialState.phase).toBe("idle");
  });

  it("follows the voice happy path to idle", () => {
    const s = run([
      { type: "MIC_START" },
      { type: "AUDIO_END" },
      { type: "TRANSCRIPT_FINAL", text: "  open my browser " },
      { type: "INTENT_READY" },
      { type: "PLAN_READY" },
      { type: "ACTION_DONE" },
      { type: "VERIFIED" },
    ]);
    expect(s.phase).toBe("speaking");
    expect(s.utterance).toBe("open my browser");
    expect(transition(s, { type: "SPEECH_DONE" }).phase).toBe("idle");
  });

  it("treats an empty transcript as nothing said", () => {
    const s = run([{ type: "MIC_START" }, { type: "TRANSCRIPT_FINAL", text: "   " }]);
    expect(s.phase).toBe("idle");
    expect(s.utterance).toBeNull();
  });

  describe("confirmation gate", () => {
    it("waits for confirmation when the plan requires it", () => {
      const s = run(toConfirmation);
      expect(s.phase).toBe("awaiting_confirmation");
      expect(s.pendingConfirmationId).toBe("c-1");
    });

    it("executes only with the matching confirmation id", () => {
      const waiting = run(toConfirmation);
      expect(transition(waiting, { type: "CONFIRMED", confirmationId: "c-0" }).phase).toBe(
        "awaiting_confirmation",
      );
      expect(transition(waiting, { type: "CONFIRMED", confirmationId: "c-1" }).phase).toBe(
        "executing",
      );
    });

    it("does not treat speech or silence as consent", () => {
      const waiting = run(toConfirmation);
      for (const e of [
        { type: "TRANSCRIPT_FINAL", text: "yes" },
        { type: "TEXT_SUBMITTED", text: "yes" },
        { type: "MIC_START" },
        { type: "AUDIO_END" },
        { type: "ACTION_DONE" },
      ] as ConversationEvent[]) {
        expect(transition(waiting, e).phase).toBe("awaiting_confirmation");
      }
    });

    it("ignores a duplicate confirmation after execution has started", () => {
      const executing = run([...toConfirmation, { type: "CONFIRMED", confirmationId: "c-1" }]);
      const after = run(
        [{ type: "ACTION_DONE" }, { type: "CONFIRMED", confirmationId: "c-1" }],
        executing,
      );
      expect(after.phase).toBe("verifying");
    });

    it("clears the confirmation id once the gate is left", () => {
      const declined = run([...toConfirmation, { type: "DECLINED" }]);
      expect(declined.phase).toBe("speaking");
      expect(declined.pendingConfirmationId).toBeNull();
      // A late confirmation for the declined action must not execute anything.
      expect(transition(declined, { type: "CONFIRMED", confirmationId: "c-1" }).phase).toBe(
        "speaking",
      );
    });
  });

  describe("stop and interruption", () => {
    it("cancels from every active phase", () => {
      for (const phase of PHASES.filter((p) => p !== "idle")) {
        const s = transition({ ...initialState, phase }, { type: "STOP" });
        expect(s.phase, phase).toBe("cancelled");
      }
    });

    it("keeps nothing pending after cancellation", () => {
      const s = run([...toConfirmation, { type: "STOP" }]);
      expect(s.pendingConfirmationId).toBeNull();
      expect(transition(s, { type: "CONFIRMED", confirmationId: "c-1" }).phase).toBe("cancelled");
      expect(transition(s, { type: "ACTION_DONE" }).phase).toBe("cancelled");
    });

    it("lets the user barge in while the assistant is speaking", () => {
      const speaking = { ...initialState, phase: "speaking" as const };
      expect(transition(speaking, { type: "MIC_START" }).phase).toBe("listening");
    });

    it("does not accept a new request mid-execution", () => {
      const executing = { ...initialState, phase: "executing" as const };
      expect(transition(executing, { type: "TEXT_SUBMITTED", text: "hi" }).phase).toBe(
        "executing",
      );
    });

    it("pauses and resumes to the same phase", () => {
      const paused = transition({ ...initialState, phase: "executing" }, { type: "PAUSE" });
      expect(paused.phase).toBe("paused");
      expect(transition(paused, { type: "RESUME" }).phase).toBe("executing");
    });
  });

  it("routes failures through recovery with a message", () => {
    const s = run([
      { type: "TEXT_SUBMITTED", text: "open notepad" },
      { type: "INTENT_READY" },
      { type: "PLAN_READY" },
      { type: "FAILED", error: "Notepad did not open." },
    ]);
    expect(s.phase).toBe("recovering");
    expect(s.error).toBe("Notepad did not open.");
  });

  it("lets a listening failure (e.g. no microphone) go to recovery", () => {
    const s = run([{ type: "MIC_START" }, { type: "FAILED", error: "No microphone." }]);
    expect(s.phase).toBe("recovering");
  });

  it("isActive is false only when idle or cancelled", () => {
    expect(PHASES.filter((p) => !isActive(p))).toEqual(["idle", "cancelled"]);
  });
});
