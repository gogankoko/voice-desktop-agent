import type { Phase } from "../core/conversation/machine";

/** Plain-language label and a non-colour symbol for every phase. */
export const PHASE_TEXT: Record<Phase, { label: string; symbol: string }> = {
  idle: { label: "Ready", symbol: "●" },
  listening: { label: "Listening…", symbol: "◉" },
  transcribing: { label: "Writing down what you said…", symbol: "✎" },
  understanding: { label: "Thinking about your request…", symbol: "…" },
  planning: { label: "Working out the steps…", symbol: "☰" },
  awaiting_confirmation: { label: "Waiting for your OK", symbol: "?" },
  executing: { label: "Doing it now…", symbol: "▶" },
  verifying: { label: "Checking it worked…", symbol: "✓?" },
  speaking: { label: "Speaking", symbol: "♪" },
  paused: { label: "Paused", symbol: "❚❚" },
  recovering: { label: "Something went wrong, sorting it out…", symbol: "!" },
  cancelled: { label: "Stopped", symbol: "■" },
};
