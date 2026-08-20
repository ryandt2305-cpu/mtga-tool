// Economy history state. Module-level signals — same pattern as economyStore.ts.
// Fetches economy snapshots for charting and re-fetches when new snapshots are saved.

import { createSignal } from "solid-js";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  Events,
  subscribe,
  getEconomyHistory,
  type EconomySnapshotRow,
  type SnapshotSavedPayload,
  type StartupCompletePayload,
} from "../lib/tauri";

// --- Signals ---

const [economyHistory, setEconomyHistory] = createSignal<EconomySnapshotRow[]>([]);
const [hasHistory, setHasHistory] = createSignal(false);

export { economyHistory, hasHistory };

// --- Internal ---

async function refreshHistory(): Promise<void> {
  try {
    const rows = await getEconomyHistory(undefined, undefined, 500);
    setEconomyHistory(rows);
    setHasHistory(rows.length > 0);
  } catch {
    // Backend not ready — will retry on next event
  }
}

// Coalesce rapid-fire snapshot events
let refreshTimer: ReturnType<typeof setTimeout> | null = null;

function scheduleRefresh(): void {
  if (refreshTimer !== null) return;
  refreshTimer = setTimeout(async () => {
    refreshTimer = null;
    await refreshHistory();
  }, 50);
}

// --- Lifecycle ---

const unlisteners: UnlistenFn[] = [];

export async function initHistoryStore(): Promise<void> {
  unlisteners.push(
    await subscribe<StartupCompletePayload>(
      Events.app.STARTUP_COMPLETE,
      () => scheduleRefresh()
    ),

    await subscribe<SnapshotSavedPayload>(
      Events.history.SNAPSHOT_SAVED,
      () => scheduleRefresh()
    )
  );

  // Hydrate from current backend state
  await refreshHistory();
}

export function cleanupHistoryStore(): void {
  for (const unlisten of unlisteners) {
    unlisten();
  }
  unlisteners.length = 0;
}
