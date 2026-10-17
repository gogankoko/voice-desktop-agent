import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { z } from "zod";

// Mirrors src-tauri/src/agent/mod.rs and commands.rs.

export const actionReportSchema = z.strictObject({
  tool: z.string(),
  ok: z.boolean(),
  verified: z.boolean(),
  summary: z.string(),
});

export const agentOutcomeSchema = z.strictObject({
  reply: z.string(),
  actions: z.array(actionReportSchema),
  cancelled: z.boolean(),
});

export const assistantSetupSchema = z.strictObject({
  configured: z.boolean(),
  model: z.string(),
});

const progressSchema = z.strictObject({
  taskId: z.number(),
  stage: z.literal("executing"),
  tool: z.string(),
});

export type ActionReport = z.infer<typeof actionReportSchema>;
export type AgentOutcome = z.infer<typeof agentOutcomeSchema>;
export type AssistantSetup = z.infer<typeof assistantSetupSchema>;

export async function runAgent(taskId: number, text: string): Promise<AgentOutcome> {
  return agentOutcomeSchema.parse(await invoke("run_agent", { taskId, text }));
}

export async function cancelAgent(): Promise<void> {
  await invoke("cancel_agent");
}

/** Calls `onExecuting` when the backend starts a tool for task `taskId`. Returns an unsubscribe. */
export async function onAgentExecuting(taskId: number, onExecuting: () => void): Promise<() => void> {
  return listen("agent-progress", (event) => {
    const p = progressSchema.safeParse(event.payload);
    if (p.success && p.data.taskId === taskId) onExecuting();
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
