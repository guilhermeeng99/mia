import { Channel, invoke } from "@tauri-apps/api/core";
import type { DownloadProgress } from "./stt";

// Typed wrappers for the optional local-LLM commands (ai_commands.rs). The UI holds
// no prompt/grammar/routing logic — it only triggers these (ADR-008, ai-commands.md).

/** A local LLM offered in the picker; mirrors Rust `ai_commands::LlmModel`. */
export interface LlmModel {
  id: string;
  label: string;
  sizeMb: number;
  downloaded: boolean;
}

/** AI feature + model status; mirrors Rust `ai_commands::AiStatus`. */
export interface AiStatus {
  enabled: boolean;
  modelInstalled: boolean;
  modelId: string;
  loaded: boolean;
  backend: string;
}

export interface CommandResult {
  newText: string;
  action: string;
}

export interface PolishResult {
  polishedText: string;
}

/** List the offered local LLMs, flagging which are installed (download gate). */
export function listLlmModels(): Promise<LlmModel[]> {
  return invoke<LlmModel[]>("list_llm_models");
}

/** AI feature + model status (drives the Settings AI section + gating). */
export function aiStatus(): Promise<AiStatus> {
  return invoke<AiStatus>("ai_status");
}

/** Download a model GGUF on demand (~2 GB), streaming progress. */
export function downloadLlm(model: string, onProgress: Channel<DownloadProgress>): Promise<void> {
  return invoke<void>("download_llm", { model, onProgress });
}

/** Free the warm LLM's RAM on demand. */
export function unloadLlm(): Promise<void> {
  return invoke<void>("unload_llm");
}

/** Command Mode: parse the spoken command and apply it to the target text. */
export function runCommand(transcript: string, target: string, lang: string): Promise<CommandResult> {
  return invoke<CommandResult>("run_command", { transcript, target, lang });
}

/** Polish: repair-constrained rewrite of the supplied text. */
export function polish(text: string, lang: string): Promise<PolishResult> {
  return invoke<PolishResult>("polish", { text, lang });
}
