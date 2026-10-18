import { describe, expect, it } from "vitest";
import { TEXT_SCALE, preferencesSchema, textSizeSchema } from "./preferences";

describe("preferencesSchema", () => {
  it("accepts every text size the backend knows", () => {
    for (const textSize of textSizeSchema.options) {
      expect(preferencesSchema.parse({ textSize, speakReplies: true, practiceMode: false })).toEqual({ textSize, speakReplies: true, practiceMode: false });
    }
  });

  it("rejects unknown sizes, raw numbers and extra fields", () => {
    expect(() => preferencesSchema.parse({ textSize: "huge", speakReplies: true, practiceMode: false })).toThrow();
    expect(() => preferencesSchema.parse({ textSize: 1.4, speakReplies: true, practiceMode: false })).toThrow();
    expect(() => preferencesSchema.parse({ textSize: "large", speakReplies: true, practiceMode: false, extra: true })).toThrow();
  });

  it("requires speakReplies to be a boolean", () => {
    expect(() => preferencesSchema.parse({ textSize: "large" })).toThrow();
    expect(() => preferencesSchema.parse({ textSize: "large", speakReplies: "yes", practiceMode: false })).toThrow();
    expect(() => preferencesSchema.parse({ textSize: "large", speakReplies: true })).toThrow();
  });

  it("has a scale for every size, growing monotonically", () => {
    const scales = textSizeSchema.options.map((s) => TEXT_SCALE[s]);
    expect(scales).toEqual([...scales].sort((a, b) => a - b));
    expect(scales[0]).toBe(1);
  });
});
