// Singleton app state. Module-level signals — no Context provider needed.
// init() subscribes to backend events, cleanup() tears down listeners.

import { createSignal } from "solid-js";
import {
  Events,
  subscribe,
  getStatus,
  getServiceStatuses,
  type CardDbLoadedPayload,
  type CardDbErrorPayload,
  type DiagnosticPayload,
  type SchemaWarningPayload,
  type ServiceStatus,
} from "../lib/tauri";
import type { UnlistenFn } from "@tauri-apps/api/event";

// --- Card database ---
const [cardDbLoaded, setCardDbLoaded] = createSignal(false);
const [cardDbCardCount, setCardDbCardCount] = createSignal(0);
const [cardDbError, setCardDbError] = createSignal<string | null>(null);

// --- Diagnostics ---
const [diagnostics, setDiagnostics] = createSignal<DiagnosticPayload[]>([]);

// --- Schema warnings ---
const [schemaWarnings, setSchemaWarnings] = createSignal<SchemaWarningPayload[]>([]);

// --- Service statuses ---
const [serviceStatuses, setServiceStatuses] = createSignal<ServiceStatus[]>([]);

async function refreshServiceStatuses(): Promise<void> {
  try {
    const statuses = await getServiceStatuses();
    setServiceStatuses(statuses);
  } catch {
    // Backend not ready yet
  }
}

// --- Exports ---
export {
  cardDbLoaded,
  cardDbCardCount,
  cardDbError,
  diagnostics,
  schemaWarnings,
  serviceStatuses,
  refreshServiceStatuses,
};

// --- Lifecycle ---

const unlisteners: UnlistenFn[] = [];

export async function initAppStore(): Promise<void> {
  unlisteners.push(
    await subscribe<CardDbLoadedPayload>(Events.cardDb.LOADED, (p) => {
      setCardDbLoaded(true);
      setCardDbCardCount(p.card_count);
      setCardDbError(null);
    }),

    await subscribe<CardDbErrorPayload>(Events.cardDb.ERROR, (p) => {
      setCardDbLoaded(false);
      setCardDbError(`${p.code}: ${p.message}`);
    }),

    await subscribe<DiagnosticPayload>(Events.diagnostics.LOGGED, (p) => {
      setDiagnostics((prev) => [...prev, p]);
    }),

    await subscribe<SchemaWarningPayload>(Events.schemaGuard.WARNING, (p) => {
      setSchemaWarnings((prev) => [...prev, p]);
    }),

    // Service status refresh on lifecycle events
    await subscribe(Events.app.STARTUP_COMPLETE, () => { refreshServiceStatuses(); }),
    await subscribe(Events.cardDb.LOADED, () => { refreshServiceStatuses(); }),
    await subscribe(Events.memory.COLLECTION_SCANNED, () => { refreshServiceStatuses(); }),
    await subscribe(Events.memory.SCAN_ERROR, () => { refreshServiceStatuses(); }),
    await subscribe(Events.log.WATCHER_STARTED, () => { refreshServiceStatuses(); }),
    await subscribe(Events.log.WATCHER_STOPPED, () => { refreshServiceStatuses(); }),
    await subscribe(Events.memory.WATCH_STARTED, () => { refreshServiceStatuses(); }),
    await subscribe(Events.memory.WATCH_STOPPED, () => { refreshServiceStatuses(); }),
    await subscribe(Events.memory.PROCESS_FOUND, () => { refreshServiceStatuses(); }),
  );

  // Hydrate from current backend state (catches events that fired before subscribe)
  try {
    const status = await getStatus();
    if (status.card_db_loaded) {
      setCardDbLoaded(true);
      setCardDbCardCount(status.card_db_card_count);
    }
  } catch {
    // Backend not ready yet — signals will update via events
  }

  // Hydrate service statuses
  await refreshServiceStatuses();
}

export function cleanupAppStore(): void {
  for (const unlisten of unlisteners) {
    unlisten();
  }
  unlisteners.length = 0;
}
