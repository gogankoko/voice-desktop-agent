import { describe, expect, it, vi } from "vitest";
import { WindowsSynthesizer } from "./windowsSynthesizer";

class FakeUtterance {
  lang = "";
  rate = 1;
  voice: unknown = null;
  onend: (() => void) | null = null;
  onerror: (() => void) | null = null;
  constructor(public text: string) {}
}
vi.stubGlobal("SpeechSynthesisUtterance", FakeUtterance);

function fakeSynth(voices: Array<Partial<SpeechSynthesisVoice>>) {
  const queue: FakeUtterance[] = [];
  return {
    queue,
    getVoices: () => voices as SpeechSynthesisVoice[],
    speak: (u: FakeUtterance) => queue.push(u),
    cancel: vi.fn(() => queue.splice(0).forEach((u) => u.onerror?.())),
    addEventListener: vi.fn(),
  };
}

const VOICES = [
  { name: "Microsoft David - English (United States)", lang: "en-US", localService: true },
  { name: "Microsoft Aria Online (Natural) - English (United States)", lang: "en-US", localService: false },
  { name: "Microsoft Hazel - English (United Kingdom)", lang: "en-GB", localService: true },
];

describe("WindowsSynthesizer", () => {
  it("prefers a natural en-US voice and speaks a little slower", () => {
    const f = fakeSynth(VOICES);
    const s = new WindowsSynthesizer(f as unknown as SpeechSynthesis);
    s.speak("Hello");
    expect((f.queue[0].voice as SpeechSynthesisVoice).name).toMatch(/Aria.*Natural/);
    expect(f.queue[0].rate).toBeLessThan(1);
  });

  it("is idle only after every queued utterance finishes", async () => {
    const f = fakeSynth(VOICES);
    const s = new WindowsSynthesizer(f as unknown as SpeechSynthesis);
    s.speak("one");
    s.speak("two");
    let idle = false;
    void s.whenIdle().then(() => (idle = true));
    f.queue[0].onend?.();
    await Promise.resolve();
    expect(idle).toBe(false);
    f.queue[1].onend?.();
    await Promise.resolve();
    expect(idle).toBe(true);
  });

  it("cancel silences speech and releases waiters", async () => {
    const f = fakeSynth(VOICES);
    const s = new WindowsSynthesizer(f as unknown as SpeechSynthesis);
    s.speak("a long sentence");
    const idle = s.whenIdle();
    s.cancel();
    await idle;
    expect(f.cancel).toHaveBeenCalled();
  });

  it("reports unavailable when Windows has no voices", () => {
    expect(new WindowsSynthesizer(fakeSynth([]) as unknown as SpeechSynthesis).available).toBe(false);
    expect(new WindowsSynthesizer(undefined).available).toBe(false);
  });
});
