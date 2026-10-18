import { beforeEach, describe, expect, it, vi } from "vitest";

const handlers: Record<string, (e: { payload: unknown }) => void> = {};
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, h: (e: { payload: unknown }) => void) => {
    handlers[name] = h;
    return () => {};
  }),
}));

import { onReminderDue, onTalkShortcut } from "./events";

describe("backend events", () => {
  beforeEach(() => {
    for (const k of Object.keys(handlers)) delete handlers[k];
  });

  it("passes on well-formed reminders only", async () => {
    const got: string[] = [];
    await onReminderDue((t) => got.push(t));
    handlers["reminder-due"]({ payload: { text: "take your pills" } });
    handlers["reminder-due"]({ payload: { text: "" } });
    handlers["reminder-due"]({ payload: { text: 5 } });
    handlers["reminder-due"]({ payload: null });
    expect(got).toEqual(["take your pills"]);
  });

  it("reports the talk shortcut", async () => {
    const cb = vi.fn();
    await onTalkShortcut(cb);
    handlers["hotkey-talk"]({ payload: null });
    expect(cb).toHaveBeenCalledOnce();
  });
});
