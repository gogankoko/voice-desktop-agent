import type { Voice } from "../ipc/preferences";
import type { SpeechSynthesizer } from "./types";

/** Plays one piece of audio; resolves when it ends or is stopped. */
export interface AudioPlayer {
  play(audio: ArrayBuffer): Promise<void>;
  stop(): void;
}

/** Plays MP3 audio through Web Audio (no media URLs, so the page's strict rules stay as they are). */
export class WebAudioPlayer implements AudioPlayer {
  private ctx: AudioContext | null = null;
  private source: AudioBufferSourceNode | null = null;
  private finish: (() => void) | null = null;

  async play(audio: ArrayBuffer): Promise<void> {
    this.ctx ??= new AudioContext();
    if (this.ctx.state === "suspended") await this.ctx.resume();
    const buffer = await this.ctx.decodeAudioData(audio);
    const ctx = this.ctx;
    await new Promise<void>((resolve) => {
      const source = ctx.createBufferSource();
      source.buffer = buffer;
      source.connect(ctx.destination);
      this.source = source;
      this.finish = resolve;
      source.onended = () => {
        if (this.source === source) this.source = null;
        resolve();
      };
      source.start();
    });
  }

  stop(): void {
    try {
      this.source?.stop();
    } catch {
      // already stopped
    }
    this.source = null;
    this.finish?.();
    this.finish = null;
  }
}

/** If the natural voice fails, use the Windows voice for this long before trying again. */
const FALLBACK_MS = 60_000;

type NaturalVoice = Exclude<Voice, "windows">;

/**
 * Reads replies aloud with a natural Azure voice, one after another. Falls back to the
 * Windows voice when the natural voice isn't chosen or set up, or fails (offline, quota),
 * so a reply is never lost. `cancel()` silences everything at once.
 */
export class ReplyVoice implements SpeechSynthesizer {
  private voice: Voice = "windows";
  private azureReady = false;
  /** Each line with the natural voice to use (null: the Windows voice). */
  private queue: Array<{ text: string; voice: NaturalVoice | null }> = [];
  private busy = false;
  private generation = 0;
  private fallbackUntil = 0;
  private idleWaiters: Array<() => void> = [];

  constructor(
    private readonly windows: SpeechSynthesizer,
    private readonly fetchAudio: (text: string, voice: NaturalVoice) => Promise<ArrayBuffer>,
    private readonly player: AudioPlayer,
    private readonly now: () => number = Date.now,
    private readonly onFallback: (why: string) => void = () => {},
  ) {}

  /** `azureReady`: the Azure Speech key is set up. */
  setVoice(voice: Voice, azureReady: boolean): void {
    this.voice = voice;
    this.azureReady = azureReady;
  }

  get available(): boolean {
    return this.windows.available || this.chosenNatural() !== null;
  }

  speak(text: string): void {
    this.enqueue(text, this.chosenNatural());
  }

  /** Says `text` with `voice` (for "Try this voice"), interrupting anything else. */
  preview(text: string, voice: Voice): void {
    this.cancel();
    this.fallbackUntil = 0;
    this.enqueue(text, voice === "windows" || !this.azureReady ? null : voice);
  }

  cancel(): void {
    this.generation++;
    this.queue = [];
    this.player.stop();
    this.windows.cancel();
    this.busy = false;
    this.flushIdle();
  }

  async whenIdle(): Promise<void> {
    if (this.busy || this.queue.length > 0) {
      await new Promise<void>((r) => this.idleWaiters.push(r));
    }
    await this.windows.whenIdle();
  }

  private chosenNatural(): NaturalVoice | null {
    if (this.voice === "windows" || !this.azureReady || this.now() < this.fallbackUntil) return null;
    return this.voice;
  }

  private enqueue(text: string, voice: NaturalVoice | null): void {
    if (!text.trim()) return;
    if (voice === null && !this.busy && this.queue.length === 0) {
      this.windows.speak(text);
      return;
    }
    this.queue.push({ text, voice });
    if (!this.busy) void this.drain(this.generation);
  }

  private async drain(gen: number): Promise<void> {
    this.busy = true;
    while (this.queue.length > 0 && gen === this.generation) {
      const { text, voice } = this.queue.shift()!;
      let spoken = false;
      if (voice && this.now() >= this.fallbackUntil) {
        try {
          const audio = await this.fetchAudio(text, voice);
          if (gen !== this.generation) return;
          await this.player.play(audio);
          spoken = true;
        } catch (e) {
          if (gen !== this.generation) return;
          this.fallbackUntil = this.now() + FALLBACK_MS;
          this.onFallback(e instanceof Error ? e.message : String(e));
        }
      }
      if (!spoken && gen === this.generation) {
        this.windows.speak(text);
        await this.windows.whenIdle();
      }
    }
    if (gen === this.generation) {
      this.busy = false;
      this.flushIdle();
    }
  }

  private flushIdle() {
    const waiters = this.idleWaiters;
    this.idleWaiters = [];
    waiters.forEach((w) => w());
  }
}
