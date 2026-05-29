import { relaunch } from "@tauri-apps/plugin-process";
import { type Update, check } from "@tauri-apps/plugin-updater";

// Signed in-app auto-update (ADR-009). The updater plugin checks the GitHub Releases
// `latest.json` endpoint and verifies the installer's minisign signature against the
// pubkey baked into tauri.conf.json before installing — so an update can never be
// tampered with in transit. Presentation calls these via the Hub; no logic lives here.

export type { Update };

/** Check GitHub Releases for a newer signed version. Returns null when up to date or
 * if the check fails (offline, rate-limited, no release yet) — never throws to the UI. */
export async function checkForUpdate(): Promise<Update | null> {
  try {
    return await check();
  } catch {
    return null;
  }
}

/** Download + install the verified update, then relaunch into the new version. */
export async function installUpdate(update: Update): Promise<void> {
  await update.downloadAndInstall();
  await relaunch();
}
