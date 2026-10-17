import { describe, expect, it } from "vitest";
import { friendlyRecognitionError } from "./errors";

describe("friendlyRecognitionError", () => {
  it.each([
    ["Microphone initialization: NotAllowedError: Permission denied", /not allowed to use the microphone/],
    ["NotFoundError: Requested device not found", /couldn't find a microphone/],
    ["WebSocket error code: 1006", /couldn't reach/],
    ["Unable to contact server. StatusCode: 1006, Unauthorized", /didn't accept the speech key/],
    ["weird internal thing", /try again/],
  ])("%s", (details, expected) => {
    expect(friendlyRecognitionError(details)).toMatch(expected);
  });

  it("never echoes raw technical details", () => {
    expect(friendlyRecognitionError("ERR_xyz at line 42 token=abc")).not.toMatch(/token|ERR_|42/);
  });
});
