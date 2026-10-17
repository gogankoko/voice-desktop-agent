/**
 * Provider-independent voice interfaces. The conversation code depends only on these,
 * so Azure, Windows voices or a future provider can be swapped without touching it.
 */

export interface SpeechRecognizer {
  /**
   * Listens for one utterance. Calls `onInterim` with the words heard so far.
   * Resolves with the final text, or "" if nothing was said or `cancel()` was called.
   * Rejects with an Error whose message is plain language, safe to show the user.
   */
  listenOnce(onInterim: (text: string) => void): Promise<string>;
  /** Stops listening and releases the microphone immediately. */
  cancel(): void;
}

export interface SpeechSynthesizer {
  /** False if no voice is installed. */
  readonly available: boolean;
  /** Queues `text`. Never rejects; failures are treated as "finished speaking". */
  speak(text: string): void;
  /** Silences current and queued speech immediately. */
  cancel(): void;
  /** Resolves once nothing is speaking or queued. */
  whenIdle(): Promise<void>;
}
