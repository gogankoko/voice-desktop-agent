import { describe, expect, it, vi } from "vitest";
import { type AudioPlayer, ReplyVoice } from "./replyVoice";
import type { SpeechSynthesizer } from "./types";

function fakeWindows() {
  const w = {
    spoken: [] as string[],
    cancels: 0,
    available: true,
    speak(t: string) {
      w.spoken.push(t);
    },
    cancel() {
      w.cancels++;
    },
    whenIdle: () => Promise.resolve(),
  };
  return w satisfies SpeechSynthesizer;
}

function fakePlayer() {
  let release: (() => void) | null = null;
  const p = {
    played: [] as string[],
    stops: 0,
    play(audio: ArrayBuffer) {
      return new Promise<void>((r) => {
        p.played.push(new TextDecoder().decode(audio));
        release = r;
      });
    },
    stop() {
      p.stops++;
      release?.();
    },
    finish() {
      release?.();
    },
  };
  return p satisfies AudioPlayer;
}

const enc = (s: string) => new TextEncoder().encode(s).buffer as ArrayBuffer;
const tick = () => new Promise((r) => setTimeout(r, 0));

describe("ReplyVoice", () => {
  it("speaks with the natural voice when chosen and set up, in order", async () => {
    const win = fakeWindows();
    const player = fakePlayer();
    const fetchAudio = vi.fn(async (t: string, v: string) => enc(`${v}:${t}`));
    const voice = new ReplyVoice(win, fetchAudio, player);
    voice.setVoice("ava", true);
    voice.speak("Hello.");
    voice.speak("Second.");
    await tick();
    expect(player.played).toEqual(["ava:Hello."]);
    player.finish();
    await tick();
    await tick();
    expect(player.played).toEqual(["ava:Hello.", "ava:Second."]);
    player.finish();
    await voice.whenIdle();
    expect(win.spoken).toEqual([]);
  });

  it("uses the Windows voice when chosen or when Azure isn't set up", () => {
    const win = fakeWindows();
    const fetchAudio = vi.fn(async () => enc("x"));
    const voice = new ReplyVoice(win, fetchAudio, fakePlayer());
    voice.setVoice("windows", true);
    voice.speak("One.");
    voice.setVoice("ava", false);
    voice.speak("Two.");
    expect(win.spoken).toEqual(["One.", "Two."]);
    expect(fetchAudio).not.toHaveBeenCalled();
  });

  it("falls back to the Windows voice when the natural voice fails, then for a minute", async () => {
    let now = 1000;
    const win = fakeWindows();
    const fetchAudio = vi.fn(async (): Promise<ArrayBuffer> => {
      throw new Error("offline");
    });
    const voice = new ReplyVoice(win, fetchAudio, fakePlayer(), () => now);
    voice.setVoice("emma", true);
    voice.speak("Still said.");
    await voice.whenIdle();
    expect(win.spoken).toEqual(["Still said."]);
    voice.speak("Straight to Windows.");
    expect(fetchAudio).toHaveBeenCalledTimes(1);
    expect(win.spoken).toEqual(["Still said.", "Straight to Windows."]);
    now += 61_000;
    voice.speak("Tries again.");
    await voice.whenIdle();
    expect(fetchAudio).toHaveBeenCalledTimes(2);
  });

  it("cancel silences everything, including what was queued", async () => {
    const win = fakeWindows();
    const player = fakePlayer();
    const voice = new ReplyVoice(win, async (t) => enc(t), player);
    voice.setVoice("ava", true);
    voice.speak("A long reply.");
    voice.speak("Never said.");
    await tick();
    voice.cancel();
    await voice.whenIdle();
    await tick();
    expect(player.played).toEqual(["A long reply."]);
    expect(player.stops).toBe(1);
    expect(win.cancels).toBe(1);
    expect(win.spoken).toEqual([]);
  });

  it("previews a chosen voice without changing the setting", async () => {
    const win = fakeWindows();
    const player = fakePlayer();
    const fetchAudio = vi.fn(async (t: string, v: string) => enc(`${v}:${t}`));
    const voice = new ReplyVoice(win, fetchAudio, player);
    voice.setVoice("ava", true);
    voice.preview("Hello from Ryan.", "ryan");
    await tick();
    player.finish();
    await voice.whenIdle();
    voice.speak("Back to Ava.");
    await tick();
    player.finish();
    await voice.whenIdle();
    expect(player.played).toEqual(["ryan:Hello from Ryan.", "ava:Back to Ava."]);
  });
});
