import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { z } from "zod";

// Mirrors src-tauri/src/agent/mod.rs and commands.rs.

export const actionReportSchema = z.strictObject({
  tool: z.string(),
  ok: z.boolean(),
  verified: z.boolean(),
  practice: z.boolean(),
  summary: z.string(),
});

export const confirmationRequestSchema = z.strictObject({
  id: z.string().min(1),
  action: z.string(),
  details: z.array(z.strictObject({ label: z.string(), value: z.string() })),
  practice: z.boolean(),
  expiresInSecs: z.number().int().positive(),
});

export const agentOutcomeSchema = z.strictObject({
  reply: z.string(),
  actions: z.array(actionReportSchema),
  cancelled: z.boolean(),
  confirmation: confirmationRequestSchema.nullable(),
});

export const assistantSetupSchema = z.strictObject({
  configured: z.boolean(),
  model: z.string(),
});

const progressSchema = z.strictObject({
  taskId: z.number(),
  stage: z.literal("executing"),
  tool: z.string(),
  detail: z.string(),
});

export type ActionReport = z.infer<typeof actionReportSchema>;
export type AgentOutcome = z.infer<typeof agentOutcomeSchema>;
export type ConfirmationRequest = z.infer<typeof confirmationRequestSchema>;
export type AssistantSetup = z.infer<typeof assistantSetupSchema>;

export async function runAgent(taskId: number, text: string): Promise<AgentOutcome> {
  return agentOutcomeSchema.parse(await invoke("run_agent", { taskId, text }));
}

/** The user's explicit answer to a confirmation question. */
export async function confirmAgent(
  taskId: number,
  confirmationId: string,
  approved: boolean,
): Promise<AgentOutcome> {
  return agentOutcomeSchema.parse(await invoke("confirm_agent", { taskId, confirmationId, approved }));
}

export async function cancelAgent(): Promise<void> {
  await invoke("cancel_agent");
}

/** Calls `onExecuting` when the backend starts a tool for task `taskId`. Returns an unsubscribe. */
/** Calls `onStep` with a plain description each time the backend starts a step for task `taskId`. */
export async function onAgentExecuting(
  taskId: number,
  onStep: (detail: string) => void,
): Promise<() => void> {
  return listen("agent-progress", (event) => {
    const p = progressSchema.safeParse(event.payload);
    if (p.success && p.data.taskId === taskId) onStep(p.data.detail);
  });
}

export async function getAssistantSetup(): Promise<AssistantSetup> {
  return assistantSetupSchema.parse(await invoke("get_assistant_setup"));
}

export async function saveGeminiKey(key: string): Promise<AssistantSetup> {
  return assistantSetupSchema.parse(await invoke("save_gemini_key", { key }));
}

export async function clearGeminiKey(): Promise<AssistantSetup> {
  return assistantSetupSchema.parse(await invoke("clear_gemini_key"));
}
