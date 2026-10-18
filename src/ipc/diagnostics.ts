import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

export async function getDiagnostics(): Promise<string> {
  return z.string().parse(await invoke("get_diagnostics"));
}

export async function clearDiagnostics(): Promise<void> {
  await invoke("clear_diagnostics");
}

/** Records a UI error in the local, redacted diagnostics log. Never throws. */
export function logUiError(message: string): void {
  void invoke("log_frontend_error", { message: message.slice(0, 1000) }).catch(() => {});
}

/** Sends uncaught UI errors to the diagnostics log (message and source only, no app data). */
export function installGlobalErrorLogging(): void {
  window.addEventListener("error", (e) => logUiError(`uncaught: ${e.message} at ${e.lineno}:${e.colno}`));
  window.addEventListener("unhandledrejection", (e) =>
    logUiError(`unhandled rejection: ${e.reason instanceof Error ? e.reason.message : typeof e.reason}`),
  );
}
