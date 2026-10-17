import { describe, expect, it } from "vitest";
import { matchIntent } from "./intents";

describe("matchIntent", () => {
  it.each([
    "Open my browser",
    "open my browser.",
    "Please open the web browser",
    "Could you open my browser for me?",
    "start the internet",
    "Launch browser please!",
  ])("recognises %j", (text) => {
    expect(matchIntent(text)).toEqual({ kind: "open_app", app: "default_browser" });
  });

  it.each([
    "",
    "browser",
    "open my email",
    "open my browser and buy milk",
    "don't open my browser",
    "open my browser; delete my files",
    "close my browser",
  ])("does not recognise %j", (text) => {
    expect(matchIntent(text)).toBeNull();
  });
});
