import { describe, expect, it } from "vitest";
import { interpretAnswer } from "./consent";

describe("interpretAnswer", () => {
  it.each(["Yes", "yes please.", "Yeah!", "Go ahead", "Yes, send it", "Do it", "Okay, go ahead"])(
    "%j is a clear yes",
    (t) => expect(interpretAnswer(t)).toBe("yes"),
  );

  it.each(["No", "no thanks", "Don't send it", "Stop", "Cancel", "No, wait", "not now"])(
    "%j is a no",
    (t) => expect(interpretAnswer(t)).toBe("no"),
  );

  it.each([
    "",
    "   ",
    "yes no",
    "yes but don't send it to Anna, send it to Bob instead",
    "maybe",
    "um yes",
    "I think so",
    "what did you say",
    "sure",
    "okay",
    "yes and also delete my files",
    "ye",
    "sounds good I guess",
  ])("%j is never taken as yes", (t) => {
    expect(interpretAnswer(t)).not.toBe("yes");
  });
});
