import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

// Mirrors src-tauri/src/status.rs. Responses are validated rather than trusted.
const availability = z.enum(["not_implemented", "mock", "available"]);

export const appStatusSchema = z.object({
  appVersion: z.string(),
  os: z.string(),
  speechRecognition: availability,
  speechSynthesis: availability,
  reasoningModel: availability,
  computerControl: availability,
});

export type AppStatus = z.infer<typeof appStatusSchema>;
export type Availability = z.infer<typeof availability>;

export async function getAppStatus(): Promise<AppStatus> {
  return appStatusSchema.parse(await invoke("get_app_status"));
}
