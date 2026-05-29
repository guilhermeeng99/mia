import { invoke } from "@tauri-apps/api/core";

/** Local-only usage stats for the Hub dashboard; mirrors Rust `stats::UsageStatsView`. */
export interface UsageStats {
  totalWords: number;
  totalMs: number;
  avgWpm: number;
  dayStreak: number;
  bestStreak: number;
}

/** Read the local usage stats (words, average WPM, day streak). Never uploaded. */
export function getStats(): Promise<UsageStats> {
  return invoke<UsageStats>("get_stats");
}

/** Clear local usage stats. */
export function resetStats(): Promise<void> {
  return invoke<void>("reset_stats");
}
