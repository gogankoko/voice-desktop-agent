/**
 * Splits text into speakable parts of roughly `max` characters, ending on sentence
 * boundaries where possible, so long articles can be read part by part and paused.
 */
export function splitIntoChunks(text: string, max = 350): string[] {
  const sentences = text
    .replace(/\s+/g, " ")
    .trim()
    .match(/[^.!?]+[.!?]+["')\]]*\s*|[^.!?]+$/g);
  if (!sentences) return [];
  const chunks: string[] = [];
  let current = "";
  for (const raw of sentences) {
    const s = raw.trim();
    if (!s) continue;
    if (s.length > max) {
      if (current) chunks.push(current);
      current = "";
      // A single very long "sentence" (e.g. a list without full stops): split on spaces.
      let rest = s;
      while (rest.length > max) {
        const cut = rest.lastIndexOf(" ", max);
        const at = cut > max / 2 ? cut : max;
        chunks.push(rest.slice(0, at).trim());
        rest = rest.slice(at).trim();
      }
      current = rest;
    } else if ((current + " " + s).trim().length > max) {
      chunks.push(current);
      current = s;
    } else {
      current = (current + " " + s).trim();
    }
  }
  if (current) chunks.push(current);
  return chunks;
}
