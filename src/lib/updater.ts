import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdateInfo = {
  version: string;
  currentVersion: string;
  notes: string | null;
  date: string | null;
};

export type DownloadProgress = {
  downloaded: number;
  total: number | null;
};

export async function checkForUpdate(): Promise<{ update: Update; info: UpdateInfo } | null> {
  try {
    const update = await check();
    if (!update) return null;
    return {
      update,
      info: {
        version: update.version,
        currentVersion: update.currentVersion,
        notes: update.body ?? null,
        date: update.date ?? null,
      },
    };
  } catch (err) {
    console.warn("Updater check failed:", err);
    return null;
  }
}

export async function downloadAndInstall(
  update: Update,
  onProgress: (p: DownloadProgress) => void,
): Promise<void> {
  let downloaded = 0;
  let total: number | null = null;
  await update.downloadAndInstall((event) => {
    switch (event.event) {
      case "Started":
        total = event.data.contentLength ?? null;
        onProgress({ downloaded: 0, total });
        break;
      case "Progress":
        downloaded += event.data.chunkLength;
        onProgress({ downloaded, total });
        break;
      case "Finished":
        onProgress({ downloaded: total ?? downloaded, total });
        break;
    }
  });
}

export async function relaunchApp(): Promise<void> {
  await relaunch();
}
