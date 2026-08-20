// Connection status signals — absorbs system lifecycle events from the feed.
// Drives a persistent status bar in Feed.tsx instead of noisy feed entries.

import { createSignal } from "solid-js";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  Events,
  subscribe,
  getStatus,
  type CollectionScannedPayload,
  type MemoryWatchStartedPayload,
  type MemoryWatchStoppedPayload,
  type WatcherStartedPayload,
  type WatcherStoppedPayload,
  type CardDbLoadedPayload,
  type OfflineHydratedPayload,
} from "../lib/tauri";

// --- Signals ---

const [logWatching, setLogWatching] = createSignal(false);
const [memoryWatching, setMemoryWatching] = createSignal(false);
const [cardDbLoaded, setCardDbLoaded] = createSignal(false);
const [collectionSize, setCollectionSize] = createSignal(0);
const [hasOfflineData, setHasOfflineData] = createSignal(false);
const [lastSyncTimestamp, setLastSyncTimestamp] = createSignal<number | null>(null);

// Refreshes derived age labels roughly once per minute so the "(Xh ago)"
// suffix stays current without state churn. Set inside initConnectionStore.
const [nowTick, setNowTick] = createSignal(Date.now());
let nowTimer: ReturnType<typeof setInterval> | null = null;

export {
  logWatching,
  memoryWatching,
  cardDbLoaded,
  collectionSize,
  hasOfflineData,
  lastSyncTimestamp,
};

// --- Derived ---

export type ConnectionLevel = "live" | "offline" | "disconnected";

export function connectionLevel(): ConnectionLevel {
  if (memoryWatching()) return "live";
  if (hasOfflineData() || collectionSize() > 0) return "offline";
  return "disconnected";
}

function formatAge(ts: number | null): string | null {
  if (ts == null) return null;
  const diffMs = Math.max(0, nowTick() - ts);
  const minutes = Math.floor(diffMs / 60_000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

export function offlineAgeLabel(): string | null {
  return formatAge(lastSyncTimestamp());
}

export function connectionLabel(): string {
  const level = connectionLevel();
  if (level === "live") {
    const size = collectionSize();
    return size > 0
      ? `Connected — watching collection (${size.toLocaleString()} cards)`
      : "Connected — watching collection";
  }
  if (level === "offline") {
    const age = offlineAgeLabel();
    return age
      ? `Offline — showing data from last sync (${age})`
      : "Offline — showing data from last sync";
  }
  return "Waiting for MTGA — open the game to sync your collection";
}

// --- Lifecycle ---

const unlisteners: UnlistenFn[] = [];

export async function initConnectionStore(): Promise<void> {
  if (nowTimer == null) {
    nowTimer = setInterval(() => setNowTick(Date.now()), 60_000);
  }

  unlisteners.push(
    await subscribe<CollectionScannedPayload>(
      Events.memory.COLLECTION_SCANNED,
      (payload) => setCollectionSize(payload.unique_cards)
    ),

    await subscribe<MemoryWatchStartedPayload>(
      Events.memory.WATCH_STARTED,
      () => {
        setMemoryWatching(true);
        // Any subsequent live scan will refresh lastSyncTimestamp on WATCH_STOPPED.
      }
    ),

    await subscribe<MemoryWatchStoppedPayload>(
      Events.memory.WATCH_STOPPED,
      () => {
        setMemoryWatching(false);
        // MemoryService keeps the live cache after WATCH_STOPPED — treat "now"
        // as the last-known-good sync time for the offline badge.
        if (collectionSize() > 0 || hasOfflineData()) {
          setHasOfflineData(true);
          setLastSyncTimestamp(Date.now());
        }
      }
    ),

    await subscribe<OfflineHydratedPayload>(
      Events.memory.OFFLINE_HYDRATED,
      (payload) => {
        setHasOfflineData(true);
        setLastSyncTimestamp(payload.snapshot_timestamp * 1000);
        if (payload.unique_cards > 0) setCollectionSize(payload.unique_cards);
      }
    ),

    await subscribe<WatcherStartedPayload>(
      Events.log.WATCHER_STARTED,
      () => setLogWatching(true)
    ),

    await subscribe<WatcherStoppedPayload>(
      Events.log.WATCHER_STOPPED,
      () => setLogWatching(false)
    ),

    await subscribe<CardDbLoadedPayload>(
      Events.cardDb.LOADED,
      () => setCardDbLoaded(true)
    ),
  );

  // Hydrate from current backend state — events may have fired before
  // subscriptions were ready (race condition during startup).
  try {
    const status = await getStatus();
    if (status.card_db_loaded) setCardDbLoaded(true);
    if (status.log_watcher_running) setLogWatching(true);
    if (status.is_watching) setMemoryWatching(true);
    if (status.collection_unique > 0) setCollectionSize(status.collection_unique);
    // If the backend has a collection but isn't watching, we're in offline
    // hydration mode. The OFFLINE_HYDRATED event may have fired before we
    // subscribed; mark offline data present so the label reflects it. The
    // timestamp stays null (unknown age) unless the event lands again.
    if (status.has_collection && !status.is_watching) {
      setHasOfflineData(true);
    }
  } catch {
    // Backend not ready yet — signals will update via events
  }
}

export function cleanupConnectionStore(): void {
  for (const unlisten of unlisteners) {
    unlisten();
  }
  unlisteners.length = 0;
  if (nowTimer != null) {
    clearInterval(nowTimer);
    nowTimer = null;
  }
}
