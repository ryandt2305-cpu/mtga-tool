// Image disk cache store — manages local card image state.
// Images downloaded by Rust ImageCacheService, served via Tauri asset protocol.

import { createSignal } from "solid-js";
import { convertFileSrc } from "@tauri-apps/api/core";
import {
  getImageCacheStatus,
  syncImageCache as invokeSyncImageCache,
  subscribe,
  Events,
  type ImageCacheProgressPayload,
  type ImageSyncEntry,
} from "../lib/tauri";

const [cacheDir, setCacheDir] = createSignal<string | null>(null);
const [cachedFiles, setCachedFiles] = createSignal<Set<string>>(new Set());
const [syncProgress, setSyncProgress] = createSignal<{
  completed: number;
  total: number;
} | null>(null);

/** Check if a local cached image exists. Returns asset:// URL or null. */
function getLocalImageUrl(
  grpId: number,
  version: "normal" | "small",
): string | null {
  const dir = cacheDir();
  if (!dir) return null;
  const filename = `${grpId}_${version}.jpg`;
  if (!cachedFiles().has(filename)) return null;
  return convertFileSrc(`${dir}/${filename}`);
}

/** Queue a batch of images for background download. Fire-and-forget. */
function syncImageCache(entries: ImageSyncEntry[]): void {
  if (entries.length === 0) return;
  invokeSyncImageCache(entries).catch((err) => {
    console.warn("Image cache sync request failed:", err);
  });
}

async function initImageCacheStore(): Promise<void> {
  try {
    const status = await getImageCacheStatus();
    setCacheDir(status.cache_dir);

    // Populate cached files set from Rust scan
    if (status.cached_files.length > 0) {
      setCachedFiles(new Set(status.cached_files));
    }
  } catch (err) {
    console.warn("Image cache init failed (non-fatal):", err);
    return;
  }

  // Listen for progress events — add newly downloaded files to the set
  subscribe<ImageCacheProgressPayload>(
    Events.imageCache.PROGRESS,
    (payload) => {
      setSyncProgress({ completed: payload.completed, total: payload.total });
      if (payload.new_files.length > 0) {
        setCachedFiles((prev) => {
          const next = new Set(prev);
          for (const f of payload.new_files) next.add(f);
          return next;
        });
      }
    },
  );
}

export {
  cacheDir,
  cachedFiles,
  syncProgress,
  getLocalImageUrl,
  syncImageCache,
  initImageCacheStore,
};
