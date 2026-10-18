import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

// Mirrors src-tauri/src/speech.rs. The Azure resource key is sent to Rust once, on save,
// and is never returned; the frontend only receives short-lived tokens.

export const speechSetupSchema = z.strictObject({
  configured: z.boolean(),
  region: z.string().nullable(),
});

const speechTokenSchema = z.strictObject({
  token: z.string().min(1),
  region: z.string().min(1),
});

export type SpeechSetup = z.infer<typeof speechSetupSchema>;

export async function getSpeechSetup(): Promise<SpeechSetup> {
  return speechSetupSchema.parse(await invoke("get_speech_setup"));
}

export async function saveAzureSpeech(region: string, key: string): Promise<SpeechSetup> {
  return speechSetupSchema.parse(await invoke("save_azure_speech", { region, key }));
}

export async function clearAzureSpeech(): Promise<SpeechSetup> {
  return speechSetupSchema.parse(await invoke("clear_azure_speech"));
}

/** Natural-voice audio (MP3) for `text`, made by Azure through the Rust side. */
export async function speakNatural(text: string, voice: string): Promise<ArrayBuffer> {
  const audio = await invoke<ArrayBuffer>("speak_natural", { text, voice });
  if (!(audio instanceof ArrayBuffer) || audio.byteLength === 0) throw new Error("No audio received.");
  return audio;
}

export async function getSpeechToken(): Promise<z.infer<typeof speechTokenSchema>> {
  return speechTokenSchema.parse(await invoke("get_speech_token"));
}
