// @vitest-environment jsdom
import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { afterEach, describe, expect, it } from "vitest";
import type { SpeechSynthesizer } from "../voice/types";
import { Reader } from "./Reader";

/** A synthesizer whose "finished speaking" moment the test controls. */
class ManualSynth implements SpeechSynthesizer {
  readonly available = true;
  spoken: string[] = [];
  cancels = 0;
  private waiters: Array<() => void> = [];
  speak(text: string) {
    this.spoken.push(text);
  }
  cancel() {
    this.cancels++;
    this.flush();
  }
  whenIdle() {
    return new Promise<void>((r) => this.waiters.push(r));
  }
  async finishCurrent() {
    await act(async () => this.flush());
  }
  private flush() {
    const w = this.waiters;
    this.waiters = [];
    w.forEach((f) => f());
  }
}

const TEXT = "First part of the article. ".repeat(12) + "Second part begins here. ".repeat(12);

afterEach(cleanup);

describe("Reader", () => {
  it("reads part by part", async () => {
    const synth = new ManualSynth();
    render(<Reader title="Library hours" text={TEXT} synth={synth} onClose={() => {}} />);
    expect(synth.spoken).toHaveLength(1);
    expect(screen.getByText(/Part 1 of/)).toBeTruthy();
    await synth.finishCurrent();
    expect(synth.spoken).toHaveLength(2);
    expect(screen.getByText(/Part 2 of/)).toBeTruthy();
  });

  it("pause stops speaking without skipping; continue repeats the interrupted part", async () => {
    const synth = new ManualSynth();
    render(<Reader title="Library hours" text={TEXT} synth={synth} onClose={() => {}} />);
    const first = synth.spoken[0];
    await userEvent.click(screen.getByRole("button", { name: "Pause" }));
    expect(synth.cancels).toBeGreaterThan(0);
    expect(screen.getByText(/Part 1 of/)).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(synth.spoken.at(-1)).toBe(first);
  });

  it("stop reading closes and silences", async () => {
    const synth = new ManualSynth();
    let closed = false;
    const { unmount } = render(
      <Reader title="x" text={TEXT} synth={synth} onClose={() => (closed = true)} />,
    );
    await userEvent.click(screen.getByRole("button", { name: "Stop reading" }));
    expect(closed).toBe(true);
    unmount();
    expect(synth.cancels).toBeGreaterThan(0);
  });

  it("has no axe violations", async () => {
    const synth = new ManualSynth();
    const { container } = render(<Reader title="x" text={TEXT} synth={synth} onClose={() => {}} />);
    const r = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
    expect(r.violations.map((v) => v.id)).toEqual([]);
  });
});
