import { invoke } from "@tauri-apps/api/core";

export interface HistoryEntry {
  id: string;
  text: string;
  createdAtMs: number;
  wordCount: number;
}

export function listHistory(): Promise<HistoryEntry[]> {
  return invoke<HistoryEntry[]>("list_history");
}

export function copyHistoryEntry(id: string): Promise<void> {
  return invoke<void>("copy_history_entry", { id });
}

export function deleteHistoryEntry(id: string): Promise<void> {
  return invoke<void>("delete_history_entry", { id });
}

export function clearHistory(): Promise<void> {
  return invoke<void>("clear_history");
}

/** Writes the full history as JSON into Downloads and reveals it in Explorer; resolves to the file path. */
export function exportHistory(): Promise<string> {
  return invoke<string>("export_history");
}
