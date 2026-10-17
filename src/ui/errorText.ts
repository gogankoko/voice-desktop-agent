/** Backend command errors are plain-language strings; anything else gets the fallback. */
export function errorText(e: unknown, fallback: string): string {
  if (typeof e === "string" && e.length > 0) return e;
  if (e instanceof Error && e.message) return e.message;
  return fallback;
}
