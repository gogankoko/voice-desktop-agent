/** A short, gentle two-note chime (for reminders). Silent where audio isn't available. */
export function playChime(): void {
  const Ctx = globalThis.AudioContext;
  if (!Ctx) return;
  try {
    const ctx = new Ctx();
    [660, 880, 660, 880].forEach((freq, i) => {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      const start = ctx.currentTime + i * 0.35;
      osc.frequency.value = freq;
      gain.gain.setValueAtTime(0.0001, start);
      gain.gain.exponentialRampToValueAtTime(0.4, start + 0.03);
      gain.gain.exponentialRampToValueAtTime(0.0001, start + 0.3);
      osc.connect(gain).connect(ctx.destination);
      osc.start(start);
      osc.stop(start + 0.32);
    });
    setTimeout(() => void ctx.close(), 2000);
  } catch {
    // No sound device: the reminder is still shown and spoken.
  }
}
