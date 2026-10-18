import { describe, expect, it } from "vitest";
import { type ConversationEvent, type ConversationState, initialState, transition } from "./conversation/machine";
import { runAgentRequest } from "./agentTurn";
import type { ActionReport, AgentOutcome } from "../ipc/agent";

const OPENED: ActionReport = {
  tool: "open_application",
  ok: true,
  verified: true,
  summary: "Opened Microsoft Edge (checked: it's in front)",
};

function harness(run: (h: { fireExecuting: () => void }) => Promise<AgentOutcome>) {
  let state: ConversationState = transition(initialState, { type: "TEXT_SUBMITTED", text: "x" });
  let current = true;
  let executingCb: (() => void) | null = null;
  let unsubscribed = false;
  const said: string[] = [];
  const notes: ActionReport[] = [];
  const phases: string[] = [];
  const h = {
    said,
    notes,
    phases,
    get state() {
      return state;
    },
    get unsubscribed() {
      return unsubscribed;
    },
    stop() {
      current = false;
      state = transition(state, { type: "STOP" });
    },
    fireExecuting: () => executingCb?.(),
  };
  const done = runAgentRequest("open my browser", {
    runAgent: () => run(h),
    onExecuting: async (cb) => {
      executingCb = cb;
      return () => (unsubscribed = true);
    },
    dispatch: (e: ConversationEvent) => {
      state = transition(state, e);
      phases.push(state.phase);
    },
    say: (t) => said.push(t),
    note: (a) => notes.push(a),
    isCurrent: () => current,
  });
  return { h, done };
}

describe("runAgentRequest", () => {
  it("plain conversation: speaks the reply, no actions", async () => {
    const { h, done } = harness(async () => ({ reply: "Hello!", actions: [], cancelled: false }));
    await done;
    expect(h.said).toEqual(["Hello!"]);
    expect(h.notes).toEqual([]);
    expect(h.state.phase).toBe("speaking");
    expect(h.unsubscribed).toBe(true);
  });

  it("verified action: shows executing while running, then success", async () => {
    const { h, done } = harness(async ({ fireExecuting }) => {
      fireExecuting();
      expect(h.state.phase).toBe("executing");
      return { reply: "Your browser is open.", actions: [OPENED], cancelled: false };
    });
    await done;
    expect(h.phases).toEqual(["planning", "executing", "verifying", "speaking"]);
    expect(h.notes).toEqual([OPENED]);
    expect(h.said).toEqual(["Your browser is open."]);
  });

  it("unverified action goes through recovery even if the model sounds confident", async () => {
    const unverified = { ...OPENED, verified: false, summary: "couldn't confirm" };
    const { h, done } = harness(async () => ({ reply: "Done!", actions: [unverified], cancelled: false }));
    await done;
    expect(h.phases).toContain("recovering");
    expect(h.state.error).toMatch(/confirmed/);
    expect(h.notes).toEqual([unverified]);
  });

  it("backend errors are spoken in plain language", async () => {
    const { h, done } = harness(() => Promise.reject("Google didn't accept the assistant key."));
    await done;
    expect(h.said).toEqual(["Google didn't accept the assistant key."]);
    expect(h.state.phase).toBe("speaking");
  });

  it("after Stop: nothing more is acted on, but real side effects are disclosed", async () => {
    let finish!: (o: AgentOutcome) => void;
    const { h, done } = harness(() => new Promise((r) => (finish = r)));
    await Promise.resolve();
    h.stop();
    finish({ reply: "Your browser is open.", actions: [OPENED], cancelled: true });
    await done;
    expect(h.state.phase).toBe("cancelled");
    expect(h.notes).toEqual([OPENED]);
    expect(h.said.join(" ")).toMatch(/already started/);
    expect(h.said).not.toContain("Your browser is open.");
  });

  it("after Stop with nothing done: stays silent", async () => {
    let finish!: (o: AgentOutcome) => void;
    const { h, done } = harness(() => new Promise((r) => (finish = r)));
    await Promise.resolve();
    h.stop();
    finish({ reply: "", actions: [], cancelled: true });
    await done;
    expect(h.said).toEqual([]);
  });
});
