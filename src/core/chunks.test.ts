import { describe, expect, it } from "vitest";
import { splitIntoChunks } from "./chunks";

describe("splitIntoChunks", () => {
  it("keeps short text whole", () => {
    expect(splitIntoChunks("Hello there. How are you?")).toEqual(["Hello there. How are you?"]);
  });

  it("groups sentences up to the limit and never splits a sentence", () => {
    const text = "One two three. Four five six. Seven eight nine. Ten eleven twelve.";
    const chunks = splitIntoChunks(text, 30);
    expect(chunks).toEqual(["One two three. Four five six.", "Seven eight nine.", "Ten eleven twelve."]);
    expect(chunks.join(" ")).toBe(text);
  });

  it("splits an over-long run on spaces", () => {
    const long = Array.from({ length: 100 }, (_, i) => `word${i}`).join(" ");
    const chunks = splitIntoChunks(long, 80);
    expect(chunks.every((c) => c.length <= 80)).toBe(true);
    expect(chunks.join(" ")).toBe(long);
  });

  it("handles empty text and trailing text without punctuation", () => {
    expect(splitIntoChunks("   ")).toEqual([]);
    expect(splitIntoChunks("First sentence. Then no full stop")).toEqual(["First sentence. Then no full stop"]);
  });
});
