import { describe, expect, it } from "vitest";
import { TEXT_SCALE, preferencesSchema, textSizeSchema } from "./preferences";

describe("preferencesSchema", () => {
  it("accepts every text size the backend knows", () => {
    for (const textSize of textSizeSchema.options) {
      expect(preferencesSchema.parse({ textSize })).toEqual({ textSize });
    }
  });

  it("rejects unknown sizes, raw numbers and extra fields", () => {
    expect(() => preferencesSchema.parse({ textSize: "huge" })).toThrow();
    expect(() => preferencesSchema.parse({ textSize: 1.4 })).toThrow();
    expect(() => preferencesSchema.parse({ textSize: "large", extra: true })).toThrow();
  });

  it("has a scale for every size, growing monotonically", () => {
    const scales = textSizeSchema.options.map((s) => TEXT_SCALE[s]);
    expect(scales).toEqual([...scales].sort((a, b) => a - b));
    expect(scales[0]).toBe(1);
  });
});
