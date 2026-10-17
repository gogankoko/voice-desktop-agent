import { describe, expect, it } from "vitest";
import { TEXT_SCALE, preferencesSchema, textSizeSchema } from "./preferences";

describe("preferencesSchema", () => {
  it("accepts every text size the backend knows", () => {
    for (const textSize of textSizeSchema.options) {
      expect(preferencesSchema.parse({ textSize, speakReplies: true })).toEqual({ textSize, speakReplies: true });
    }
  });

  it("rejects unknown sizes, raw numbers and extra fields", () => {
    expect(() => preferencesSchema.parse({ textSize: "huge", speakReplies: true })).toThrow();
    expect(() => preferencesSchema.parse({ textSize: 1.4, speakReplies: true })).toThrow();
    expect(() => preferencesSchema.parse({ textSize: "large", speakReplies: true, extra: true })).toThrow();
  });

  it("requires speakReplies to be a boolean", () => {
    expect(() => preferencesSchema.parse({ textSize: "large" })).toThrow();
    expect(() => preferencesSchema.parse({ textSize: "large", speakReplies: "yes" })).toThrow();
  });

  it("has a scale for every size, growing monotonically", () => {
    const scales = textSizeSchema.options.map((s) => TEXT_SCALE[s]);
    expect(scales).toEqual([...scales].sort((a, b) => a - b));
    expect(scales[0]).toBe(1);
  });
});
