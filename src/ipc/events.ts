import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { z } from "zod";

const reminderSchema = z.object({ text: z.string().min(1).max(200) });

/** Ctrl+Alt+Space was pressed anywhere (the backend has already brought the window forward). */
export async function onTalkShortcut(cb: () => void): Promise<() => void> {
  return listen("hotkey-talk", () => cb());
}

/** A reminder or timer the person set is due. */
export async function onReminderDue(cb: (text: string) => void): Promise<() => void> {
  return listen("reminder-due", (event) => {
    const r = reminderSchema.safeParse(event.payload);
    if (r.success) cb(r.data.text);
  });
}

const wakeStatusSchema = z.strictObject({
  state: z.enum(["off", "listening", "unavailable"]),
  detail: z.string().max(300).nullable(),
});
export type WakeStatus = z.infer<typeof wakeStatusSchema>;

/** Whether the assistant is listening for its name (and why not, if it can't). */
export async function getWakeStatus(): Promise<WakeStatus> {
  return wakeStatusSchema.parse(await invoke("get_wake_status"));
}

export async function onWakeStatus(cb: (s: WakeStatus) => void): Promise<() => void> {
  return listen("wake-status", (event) => {
    const s = wakeStatusSchema.safeParse(event.payload);
    if (s.success) cb(s.data);
  });
}

/** The assistant's name was heard (the backend has already brought the window forward). */
export async function onWakeWord(cb: () => void): Promise<() => void> {
  return listen("wake-word", () => cb());
}
