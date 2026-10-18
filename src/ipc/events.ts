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
