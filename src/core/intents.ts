/**
 * Fixed-phrase intent recogniser. A stand-in until the reasoning model arrives (Phase 5);
 * it only ever maps to allowlisted tools, and anything unrecognised returns null.
 */

export type Intent = { kind: "open_app"; app: "default_browser" };

const OPEN_BROWSER =
  /^(?:(?:can|could|would) you |please )?(?:open|start|launch|bring up|show me)(?: up)? (?:my |the |a )?(?:web |internet )?(?:browser|internet)(?: (?:for me|please))*$/;

function normalise(text: string): string {
  return text
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s']/gu, " ")
    .replace(/\s+/g, " ")
    .trim();
}

export function matchIntent(text: string): Intent | null {
  if (OPEN_BROWSER.test(normalise(text))) return { kind: "open_app", app: "default_browser" };
  return null;
}
