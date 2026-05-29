import { invoke } from "@tauri-apps/api/core";

// Typed mirror of Rust `snippets::{Snippet, ExpansionResult}` (serde camelCase).
// The Hub holds no matching logic — it CRUDs snippets + previews via these wrappers.

export type SnippetAnchor = "anywhere" | "startOnly";
export type SnippetCase = "verbatim" | "matchSentence";

export interface Snippet {
  id: string;
  trigger: string;
  expansion: string;
  anchor: SnippetAnchor;
  case: SnippetCase;
  enabled: boolean;
}

export interface ExpansionResult {
  output: string;
  appliedTriggers: string[];
}

/** List all snippets. */
export function listSnippets(): Promise<Snippet[]> {
  return invoke<Snippet[]>("list_snippets");
}

/** Create (empty id) or update (existing id) a snippet; rejects duplicate triggers. */
export function upsertSnippet(snippet: Snippet): Promise<Snippet> {
  return invoke<Snippet>("upsert_snippet", { snippet });
}

/** Delete a snippet by id. */
export function deleteSnippet(id: string): Promise<void> {
  return invoke<void>("delete_snippet", { id });
}

/** Preview what the current snippets do to a sample utterance (no dictation). */
export function previewExpansion(text: string): Promise<ExpansionResult> {
  return invoke<ExpansionResult>("preview_expansion", { text });
}
