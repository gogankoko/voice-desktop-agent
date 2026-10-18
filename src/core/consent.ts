/**
 * Interprets a spoken answer to a confirmation question. Deliberately strict:
 * only an exact, unambiguous yes counts as consent. Anything with a negation or
 * hesitation, partial speech, silence, or extra words is never "yes".
 */

export type Answer = "yes" | "no" | "unclear";

const YES = new Set([
  "yes",
  "yes please",
  "yeah",
  "yep",
  "yes do it",
  "yes go ahead",
  "go ahead",
  "do it",
  "yes send it",
  "send it",
  "confirm",
  "yes confirm",
  "ok go ahead",
  "okay go ahead",
]);

const NEGATION = /\b(no|not|don't|dont|do not|never|stop|cancel|wait|hold on|nope)\b/;
const HESITATION = /\b(maybe|perhaps|i think|not sure|hmm|um|uh|what|why|who)\b/;

export function normaliseAnswer(text: string): string {
  return text
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s']/gu, " ")
    .replace(/\s+/g, " ")
    .trim();
}

export function interpretAnswer(text: string): Answer {
  const t = normaliseAnswer(text);
  if (!t) return "unclear";
  if (NEGATION.test(t)) return t.split(" ").length <= 4 ? "no" : "unclear";
  if (HESITATION.test(t)) return "unclear";
  return YES.has(t) ? "yes" : "unclear";
}
