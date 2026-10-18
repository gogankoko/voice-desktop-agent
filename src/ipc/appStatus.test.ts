import { describe, expect, it } from "vitest";
import { appStatusSchema } from "./appStatus";

const valid = {
  appVersion: "0.1.0",
  os: "windows",
  speechRecognition: "not_implemented",
  speechSynthesis: "not_implemented",
  reasoningModel: "not_implemented",
  computerControl: "not_implemented",
    talkShortcut: "Ctrl+Alt+Space",
};

describe("appStatusSchema", () => {
  it("accepts the shape the Rust command returns", () => {
    expect(appStatusSchema.parse(valid)).toEqual(valid);
  });

  it("rejects unknown availability values", () => {
    expect(() => appStatusSchema.parse({ ...valid, computerControl: "yes" })).toThrow();
  });

  it("rejects missing fields", () => {
    const { os: _os, ...missing } = valid;
    expect(() => appStatusSchema.parse(missing)).toThrow();
  });
});
