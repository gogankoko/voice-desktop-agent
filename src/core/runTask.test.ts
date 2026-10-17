import { describe, expect, it, vi } from "vitest";
import { type ConversationEvent, type ConversationState, initialState, transition } from "./conversation/machine";
import { UNRECOGNISED_REPLY, runRequest } from "./runTask";
import type { OpenResult } from "../ipc/tools";

const BROWSER = { kind: "open_app", app: "default_browser" } as const;

function harness(open: () => Promise<OpenResult>) {
  let state: ConversationState = transition(initialState, { type: "TEXT_SUBMITTED", text: "x" });
  const said: string[] = [];
  let current = true;
  const deps = {
    dispatch: (e: ConversationEvent) => (state = transition(state, e)),
    say: (t: string) => said.push(t),
    openApplication: vi.fn(open),
    isCurrent: () => current,
  };
  return {
    deps,
    said,
    get state() {
      return state;
    },
    stop() {
      current = false;
      state = transition(state, { type: "STOP" });
    },
  };
}

describe("runRequest", () => {
  it("reports success only when verified", async () => {
    const h = harness(async () => ({ app: "default_browser", verified: true, appName: "Microsoft Edge" }));
    await runRequest(BROWSER, h.deps);
    expect(h.state.phase).toBe("speaking");
    expect(h.said.at(-1)).toBe("Microsoft Edge is open and in front of you now.");
  });

  it("does not claim success when verification fails", async () => {
    const h = harness(async () => ({ app: "default_browser", verified: false, appName: "Microsoft Edge" }));
    await runRequest(BROWSER, h.deps);
    expect(h.state.phase).toBe("speaking");
    expect(h.state.error).toMatch(/confirm/);
    expect(h.said.at(-1)).toMatch(/couldn't see it appear/);
    expect(h.said.join(" ")).not.toMatch(/is open/);
  });

  it("explains backend errors in plain language", async () => {
    const h = harness(() => Promise.reject("I couldn't find your web browser."));
    await runRequest(BROWSER, h.deps);
    expect(h.state.phase).toBe("speaking");
    expect(h.said.at(-1)).toBe("I couldn't find your web browser. Nothing else was changed.");
  });

  it("does not call any tool for unrecognised requests", async () => {
    const h = harness(async () => ({ app: "default_browser", verified: true, appName: "x" }));
    await runRequest(null, h.deps);
    expect(h.deps.openApplication).not.toHaveBeenCalled();
    expect(h.said).toEqual([UNRECOGNISED_REPLY]);
    expect(h.state.phase).toBe("speaking");
  });

  it("stays cancelled and is honest when stopped mid-launch", async () => {
    let finish!: (r: OpenResult) => void;
    const h = harness(() => new Promise((r) => (finish = r)));
    const run = runRequest(BROWSER, h.deps);
    expect(h.state.phase).toBe("executing");
    h.stop();
    finish({ app: "default_browser", verified: true, appName: "Microsoft Edge" });
    await run;
    expect(h.state.phase).toBe("cancelled");
    expect(h.said.at(-1)).toMatch(/may already have opened/);
    expect(h.said.join(" ")).not.toMatch(/is open and in front/);
  });

  it("stays silent about errors that arrive after a stop", async () => {
    let fail!: (e: string) => void;
    const h = harness(() => new Promise((_, rej) => (fail = rej)));
    const run = runRequest(BROWSER, h.deps);
    h.stop();
    fail("boom");
    await run;
    expect(h.state.phase).toBe("cancelled");
    expect(h.said).not.toContain("boom Nothing else was changed.");
  });
});
