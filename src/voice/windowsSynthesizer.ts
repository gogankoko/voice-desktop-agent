import type { SpeechSynthesizer } from "./types";

/**
 * Speaks with the voices installed in Windows, through WebView2's speechSynthesis.
 * Free, on-device, nothing is sent anywhere.
 */
export class WindowsSynthesizer implements SpeechSynthesizer {
  private pending = 0;
  private idleWaiters: Array<() => void> = [];
  private voice: SpeechSynthesisVoice | null = null;

  constructor(
    private readonly synth: SpeechSynthesis | undefined = globalThis.speechSynthesis,
    private readonly lang = "en-US",
  ) {
    if (!synth) return;
    this.pickVoice();
    // Chromium loads the voice list asynchronously.
    synth.addEventListener?.("voiceschanged", () => this.pickVoice());
  }

  get available(): boolean {
    return !!this.synth && this.synth.getVoices().length > 0;
  }

  speak(text: string): void {
    if (!this.synth || !text.trim()) return;
    const u = new SpeechSynthesisUtterance(text);
    u.lang = this.lang;
    if (this.voice) u.voice = this.voice;
    u.rate = 0.95; // a touch slower than default, easier to follow
    this.pending++;
    let finished = false;
    const done = () => {
      if (finished) return;
      finished = true;
      this.pending = Math.max(0, this.pending - 1);
      if (this.pending === 0) this.flushIdle();
    };
    u.onend = done;
    u.onerror = done;
    this.synth.speak(u);
  }

  cancel(): void {
    this.synth?.cancel();
    this.pending = 0;
    this.flushIdle();
  }

  whenIdle(): Promise<void> {
    if (this.pending === 0) return Promise.resolve();
    return new Promise((r) => this.idleWaiters.push(r));
  }

  private flushIdle() {
    const waiters = this.idleWaiters;
    this.idleWaiters = [];
    waiters.forEach((w) => w());
  }

  /** Prefers a natural-sounding voice in the right language. */
  private pickVoice() {
    const voices = this.synth?.getVoices() ?? [];
    const lang = voices.filter((v) => v.lang.toLowerCase().startsWith(this.lang.slice(0, 2).toLowerCase()));
    this.voice =
      lang.find((v) => /natural/i.test(v.name) && v.lang === this.lang) ??
      lang.find((v) => v.lang === this.lang && v.localService) ??
      lang.find((v) => v.lang === this.lang) ??
      lang[0] ??
      null;
  }
}
