import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

// Mirrors src-tauri/src/tools/open_app.rs.
export const allowedAppSchema = z.enum(["default_browser"]);

export const openResultSchema = z.strictObject({
  app: allowedAppSchema,
  verified: z.boolean(),
  appName: z.string(),
});

export type AllowedApp = z.infer<typeof allowedAppSchema>;
export type OpenResult = z.infer<typeof openResultSchema>;

export async function openApplication(app: AllowedApp): Promise<OpenResult> {
  return openResultSchema.parse(
    await invoke("open_application", { app: allowedAppSchema.parse(app) }),
  );
}
