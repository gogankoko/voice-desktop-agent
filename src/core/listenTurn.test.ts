import { describe, expect, it } from "vitest";
import { type ConversationEvent, type ConversationState, initialState, transition } from "./conversation/machine";
import { NOTHING_HEARD, listenTurn } from "./listenTurn";
import { MockRecognizer } from "../voice/mock";

function harness(recognizer = new MockRecognizer(["open", "open my"])) {
  let state: ConversationState = initialState;
  let current = true;
  const interim: string[] = [];
  const heard: string[] = [];
  const said: string[] = [];
  const run = listenTurn({
    recognizer,
    dispatch: (e: ConversationEvent) => (state = transition(state, e)),
    onInterim: (t) => interim.push(t),
    onHeard: (t) => heard.push(t),
    say: (t) => said.push(t),
    isCurrent: () => current,
  });
  return {
    recognizer,
    run,
    interim,
    heard,
    said,
    get state() {
      return state;
    },
    stop() {
      current = false;
      state = transition(state, { type: "STOP" });
      recognizer.cancel();
    },
  };
}

describe("listenTurn", () => {
  it("shows interim words, then hands the final text to the conversation", async () => {
    const h = harness();
    expect(h.state.phase).toBe("listening");
    h.recognizer.hear("Open my browser.");
    await h.run;
    expect(h.interim).toEqual(["open", "open my", ""]);
    expect(h.heard).toEqual(["Open my browser."]);
    expect(h.state.phase).toBe("understanding");
    expect(h.state.utterance).toBe("Open my browser.");
  });

  it("treats silence as nothing said, not as a request", async () => {
    const h = harness();
    h.recognizer.hear("");
    await h.run;
    expect(h.state.phase).toBe("idle");
    expect(h.heard).toEqual([]);
    expect(h.said).toEqual([NOTHING_HEARD]);
  });

  it("explains provider errors in plain language and recovers", async () => {
    const h = harness();
    h.recognizer.error("I couldn't find a microphone.");
    await h.run;
    expect(h.said).toEqual(["I couldn't find a microphone."]);
    expect(h.state.phase).toBe("speaking");
    expect(h.state.error).toBe("I couldn't find a microphone.");
  });

  it("Stop releases the microphone and nothing heard afterwards is acted on", async () => {
    const h = harness();
    h.stop();
    await h.run;
    expect(h.recognizer.cancelled).toBe(1);
    expect(h.state.phase).toBe("cancelled");
    expect(h.heard).toEqual([]);
    expect(h.said).toEqual([]);
  });
});
