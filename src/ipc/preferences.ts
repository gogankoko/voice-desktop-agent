import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

// Mirrors src-tauri/src/prefs.rs.
export const textSizeSchema = z.enum(["normal", "large", "extra_large"]);
export const voiceSchema = z.enum(["ava", "andrew", "emma", "brian", "sonia", "ryan", "windows"]);

export const preferencesSchema = z.strictObject({
  textSize: textSizeSchema,
  speakReplies: z.boolean(),
  practiceMode: z.boolean(),
  fileSearch: z.boolean(),
  voice: voiceSchema,
});

export type TextSize = z.infer<typeof textSizeSchema>;
export type Voice = z.infer<typeof voiceSchema>;
export type Preferences = z.infer<typeof preferencesSchema>;

export const TEXT_SCALE: Record<TextSize, number> = {
  normal: 1,
  large: 1.2,
  extra_large: 1.4,
};

export async function getPreferences(): Promise<Preferences> {
  return preferencesSchema.parse(await invoke("get_preferences"));
}

export async function setPreferences(prefs: Preferences): Promise<Preferences> {
  return preferencesSchema.parse(
    await invoke("set_preferences", { prefs: preferencesSchema.parse(prefs) }),
  );
}
