// Economy state. Module-level signals — same pattern as collectionStore.ts.
// Manages merged inventory (log + memory) and re-fetches on any inventory event.

import { createSignal } from "solid-js";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  Events,
  subscribe,
  getMergedInventory,
  type PlayerInventory,
  type StartupCompletePayload,
  type InventoryScannedPayload,
  type InventoryChangedPayload,
  type SessionStartedPayload,
  type InventoryUpdatedPayload,
} from "../lib/tauri";

// --- Signals ---

const [inventory, setInventory] = createSignal<PlayerInventory | null>(null);
const [hasInventory, setHasInventory] = createSignal(false);

export { inventory, hasInventory };

// --- Internal ---

async function refreshInventory(): Promise<void> {
  try {
    const inv = await getMergedInventory();
    if (inv) {
      setInventory(inv);
      setHasInventory(true);
    }
  } catch {
    // Backend not ready — will retry on next event
  }
}

// Coalesce rapid-fire inventory events (e.g., log + memory firing within ms of each other)
let refreshTimer: ReturnType<typeof setTimeout> | null = null;

function scheduleRefresh(): void {
  if (refreshTimer !== null) return;
  refreshTimer = setTimeout(async () => {
    refreshTimer = null;
    await refreshInventory();
  }, 50);
}

// --- Lifecycle ---

const unlisteners: UnlistenFn[] = [];

export async function initEconomyStore(): Promise<void> {
  unlisteners.push(
    await subscribe<StartupCompletePayload>(
      Events.app.STARTUP_COMPLETE,
      (p) => {
        if (p.has_inventory) scheduleRefresh();
      }
    ),

    await subscribe<SessionStartedPayload>(
      Events.log.SESSION_STARTED,
      () => scheduleRefresh()
    ),

    await subscribe<InventoryUpdatedPayload>(
      Events.log.INVENTORY_UPDATED,
      () => scheduleRefresh()
    ),

    await subscribe<InventoryScannedPayload>(
      Events.memory.INVENTORY_SCANNED,
      () => scheduleRefresh()
    ),

    await subscribe<InventoryChangedPayload>(
      Events.memory.INVENTORY_CHANGED,
      () => scheduleRefresh()
    )
  );

  // Hydrate from current backend state
  await refreshInventory();
}

export function cleanupEconomyStore(): void {
  for (const unlisten of unlisteners) {
    unlisten();
  }
  unlisteners.length = 0;
}
