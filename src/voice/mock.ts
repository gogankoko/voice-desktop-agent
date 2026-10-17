import type { SpeechRecognizer, SpeechSynthesizer } from "./types";

/** Deterministic recognizer for tests: replays scripted interim words, then a result. */
export class MockRecognizer implements SpeechRecognizer {
  cancelled = 0;
  private settle: ((t: string) => void) | null = null;
  private fail: ((e: Error) => void) | null = null;

  constructor(private readonly interim: string[] = []) {}

  listenOnce(onInterim: (text: string) => void): Promise<string> {
    this.interim.forEach(onInterim);
    return new Promise((resolve, reject) => {
      this.settle = resolve;
      this.fail = reject;
    });
  }

  /** Test hook: the user finished speaking. */
  hear(text: string) {
    this.settle?.(text);
  }

  /** Test hook: the provider failed. */
  error(message: string) {
    this.fail?.(new Error(message));
  }

  cancel(): void {
    this.cancelled++;
    this.settle?.("");
  }
}

export class MockSynthesizer implements SpeechSynthesizer {
  readonly available = true;
  spoken: string[] = [];
  cancelled = 0;
  speak(text: string): void {
    this.spoken.push(text);
  }
  cancel(): void {
    this.cancelled++;
  }
  whenIdle(): Promise<void> {
    return Promise.resolve();
  }
}
