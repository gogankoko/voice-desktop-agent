import { describe, expect, it } from "vitest";
import { type ConversationEvent, type ConversationState, initialState, transition } from "./conversation/machine";
import { answerConfirmation, confirmationQuestion, runAgentRequest } from "./agentTurn";
import type { ActionReport, AgentOutcome, ConfirmationRequest } from "../ipc/agent";

const OPENED: ActionReport = {
  tool: "open_application",
  ok: true,
  verified: true,
  practice: false,
  summary: "Opened Microsoft Edge (checked: it's in front)",
};

const PRACTICE_SENT: ActionReport = {
  tool: "practice_send_message",
  ok: true,
  verified: true,
  practice: true,
  summary: "Practice only: pretended to send a message to Anna. Nothing was sent.",
};

const QUESTION: ConfirmationRequest = {
  id: "c1-abc",
  action: "Send a message (practice: nothing will really be sent)",
  details: [
    { label: "To", value: "Anna" },
    { label: "Message", value: "See you at five." },
  ],
  practice: true,
  expiresInSecs: 120,
};

const outcome = (o: Partial<AgentOutcome>): AgentOutcome => ({
  reply: "",
  actions: [],
  cancelled: false,
  confirmation: null,
  readAloud: null,
  ...o,
});

type Backend = (h: Harness) => Promise<AgentOutcome>;

class Harness {
  state: ConversationState = transition(initialState, { type: "TEXT_SUBMITTED", text: "x" });
  current = true;
  said: string[] = [];
  notes: ActionReport[] = [];
  asked: ConfirmationRequest[] = [];
  phases: string[] = [];
  confirmCalls: Array<[string, boolean]> = [];
  executingCb: (() => void) | null = null;
  unsubscribed = 0;
  progress: string[] = [];

  constructor(
    private run: Backend,
    private confirm: Backend = async () => outcome({}),
  ) {}

  deps = {
    runAgent: () => this.run(this),
    confirmAgent: (id: string, approved: boolean) => {
      this.confirmCalls.push([id, approved]);
      return this.confirm(this);
    },
    onExecuting: async (cb: (detail: string) => void) => {
      this.executingCb = () => cb("Opening example.org");
      return () => void this.unsubscribed++;
    },
    dispatch: (e: ConversationEvent) => {
      const before = this.state.phase;
      this.state = transition(this.state, e);
      if (this.state.phase !== before) this.phases.push(this.state.phase);
    },
    say: (t: string) => void this.said.push(t),
    note: (a: ActionReport) => void this.notes.push(a),
    askConfirmation: (c: ConfirmationRequest) => void this.asked.push(c),
    onProgress: (d: string) => void this.progress.push(d),
    isCurrent: () => this.current,
  };

  stop() {
    this.current = false;
    this.state = transition(this.state, { type: "STOP" });
  }
}

describe("runAgentRequest", () => {
  it("plain conversation: speaks the reply, no actions", async () => {
    const h = new Harness(async () => outcome({ reply: "Hello!" }));
    await runAgentRequest("hi", h.deps);
    expect(h.said).toEqual(["Hello!"]);
    expect(h.state.phase).toBe("speaking");
    expect(h.unsubscribed).toBe(1);
  });

  it("verified action: shows executing while running, then success", async () => {
    const h = new Harness(async (self) => {
      self.executingCb?.();
      expect(self.state.phase).toBe("executing");
      return outcome({ reply: "Your browser is open.", actions: [OPENED] });
    });
    await runAgentRequest("open", h.deps);
    expect(h.phases).toEqual(["planning", "executing", "verifying", "speaking"]);
    expect(h.progress).toEqual(["Opening example.org", ""]);
    expect(h.notes).toEqual([OPENED]);
  });

  it("unverified action goes through recovery even if the model sounds confident", async () => {
    const unverified = { ...OPENED, verified: false };
    const h = new Harness(async () => outcome({ reply: "Done!", actions: [unverified] }));
    await runAgentRequest("open", h.deps);
    expect(h.phases).toContain("recovering");
  });

  it("backend errors are spoken in plain language", async () => {
    const h = new Harness(() => Promise.reject("Google didn't accept the assistant key."));
    await runAgentRequest("hi", h.deps);
    expect(h.said).toEqual(["Google didn't accept the assistant key."]);
    expect(h.state.phase).toBe("speaking");
  });

  it("after Stop: real side effects are disclosed, nothing else is said", async () => {
    let finish!: (o: AgentOutcome) => void;
    const h = new Harness(() => new Promise((r) => (finish = r)));
    const done = runAgentRequest("open", h.deps);
    await Promise.resolve();
    h.stop();
    finish(outcome({ reply: "Your browser is open.", actions: [OPENED], cancelled: true }));
    await done;
    expect(h.state.phase).toBe("cancelled");
    expect(h.said.join(" ")).toMatch(/already started/);
    expect(h.said).not.toContain("Your browser is open.");
  });
});

describe("confirmations", () => {
  it("a paused request asks, and waits in awaiting_confirmation", async () => {
    const h = new Harness(async () => outcome({ confirmation: QUESTION }));
    await runAgentRequest("tell Anna", h.deps);
    expect(h.state.phase).toBe("awaiting_confirmation");
    expect(h.state.pendingConfirmationId).toBe("c1-abc");
    expect(h.asked).toEqual([QUESTION]);
    expect(h.said).toEqual([]);
  });

  it("yes: sends the exact id, executes, reports the practice action", async () => {
    const h = new Harness(
      async () => outcome({ confirmation: QUESTION }),
      async () => outcome({ reply: "Done, that was practice.", actions: [PRACTICE_SENT] }),
    );
    await runAgentRequest("tell Anna", h.deps);
    await answerConfirmation(QUESTION, true, h.deps);
    expect(h.confirmCalls).toEqual([["c1-abc", true]]);
    expect(h.notes).toEqual([PRACTICE_SENT]);
    expect(h.said).toEqual(["Done, that was practice."]);
    expect(h.state.phase).toBe("speaking");
  });

  it("no: nothing runs, the assistant acknowledges", async () => {
    const h = new Harness(
      async () => outcome({ confirmation: QUESTION }),
      async () => outcome({ reply: "" }),
    );
    await runAgentRequest("tell Anna", h.deps);
    await answerConfirmation(QUESTION, false, h.deps);
    expect(h.confirmCalls).toEqual([["c1-abc", false]]);
    expect(h.notes).toEqual([]);
    expect(h.said).toEqual(["Okay, I won't do that."]);
    expect(h.state.phase).toBe("speaking");
  });

  it("an expired yes ends cleanly without an action", async () => {
    const h = new Harness(
      async () => outcome({ confirmation: QUESTION }),
      async () => outcome({ reply: "I waited a while for your answer, so to be safe I didn't do it." }),
    );
    await runAgentRequest("tell Anna", h.deps);
    await answerConfirmation(QUESTION, true, h.deps);
    expect(h.notes).toEqual([]);
    expect(h.state.phase).toBe("speaking");
  });

  it("a result that arrives while paused is still shown", async () => {
    const h = new Harness(async (self) => {
      self.executingCb?.();
      self.deps.dispatch({ type: "PAUSE" });
      expect(self.state.phase).toBe("paused");
      return outcome({ reply: "Done.", actions: [OPENED] });
    });
    await runAgentRequest("open", h.deps);
    expect(h.state.phase).toBe("speaking");
    expect(h.said).toEqual(["Done."]);
  });

  it("a later step can ask again", async () => {
    const second = { ...QUESTION, id: "c2-def" };
    const h = new Harness(
      async () => outcome({ confirmation: QUESTION }),
      async () => outcome({ actions: [PRACTICE_SENT], confirmation: second }),
    );
    await runAgentRequest("tell Anna and Bob", h.deps);
    await answerConfirmation(QUESTION, true, h.deps);
    expect(h.state.phase).toBe("awaiting_confirmation");
    expect(h.state.pendingConfirmationId).toBe("c2-def");
    expect(h.asked).toEqual([QUESTION, second]);
  });

  it("the spoken question repeats the exact details", () => {
    const q = confirmationQuestion(QUESTION);
    expect(q).toContain("To: Anna.");
    expect(q).toContain("Message: See you at five.");
    expect(q).toMatch(/yes or no/);
  });
});
