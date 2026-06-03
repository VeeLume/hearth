// App self-update: check GitHub Releases on launch and, with the user's OK,
// download + install + relaunch. Signed updates are verified against the public
// key in tauri.conf.json before they're applied.
//
// Skipped entirely in offline mode (the Online-features master switch) so the
// "no network calls at all" promise holds. Silent on any failure — offline, no
// release yet, GitHub unreachable — so a failed check never disrupts launch.

import { check } from "@tauri-apps/plugin-updater";
import { ask } from "@tauri-apps/plugin-dialog";
import { relaunch } from "@tauri-apps/plugin-process";
import { commands } from "$lib/ipc";

export async function checkForUpdates(): Promise<void> {
  try {
    const s = await commands.getSettings();
    if (s.status === "ok" && !s.data.online_enabled) return; // offline mode

    const update = await check();
    if (!update?.available) return;

    const yes = await ask(
      `Hearth ${update.version} is available (you have ${update.currentVersion}). ` +
        `Download and install now? Hearth will restart.`,
      { title: "Update available", kind: "info" },
    );
    if (!yes) return;

    await update.downloadAndInstall();
    await relaunch();
  } catch {
    // Offline / no release / check failed — ignore; updates aren't critical.
  }
}
