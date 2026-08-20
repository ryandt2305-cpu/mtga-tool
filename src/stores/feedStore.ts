// Live feed state. Capped event queue driven by LogService + MemoryService events.
// Builder functions live in feedBuilders.ts, event handlers in feedStoreHandlers.ts.
//
// Key behaviors:
// - Post-match grouping: session_started(is_refresh=true) triggers 2s buffer,
//   absorbs rank/mastery/quest/cosmetics into a single "Match complete" entry.
// - Match-start grouping: MATCH_STARTING triggers 500ms buffer, absorbs DeckUpdate.
// - Delta-only: rank and mastery only emit feed entries when values actually change.
// - Whitelist: only specific LOG_EVENT types get handlers; all others silently dropped.
// - Connection lifecycle events go to connectionStore, not here.

import { createSignal, createRoot, type Accessor } from "solid-js";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import {
  Events,
  subscribe,
  insertFeedEntry,
  getFeedEntries,
  type FeedEntryRow,
  type InventoryUpdatedPayload,
  type SessionStartedPayload,
  type LogEventPayload,
  type MemoryCollectionChangedPayload,
  type InventoryChangedPayload,
  type DiagnosticPayload,
  type MasteryUpdatedPayload,
  type MatchStartingPayload,
  type MatchResultPayload,
  type AchievementsUpdatedPayload,
  type MemoryWatchStoppedPayload,
  type WatcherStoppedPayload,
  type LogReadyPayload,
  type PlayerInventory,
  type DeckUpdateDetails,
  type CosmeticsUpdatedPayload,
  type EventsUpdatedPayload,
} from "../lib/tauri";
import {
  buildInventoryEntry,
  buildMemoryInventoryChangedEntry,
  buildDiagnosticEntry,
} from "./feedBuilders";
import { buildSessionStartEntry } from "./feedBuildersEvent";
import {
  initHandlers,
  startSession,
  setStartupSuppression,
  accumulateDelta,
  getSessionId,
  emitSessionEnd,
  captureMatchResult,
  startMatchStartBuffer,
  startPostMatchBuffer,
  handleMasteryEvent,
  dispatchLogEvent,
  bufferCollectionChange,
  tryEarlyFlushCollection,
  cleanupHandlerTimers,
  handleCosmeticsUpdated,
  handleEventsUpdated,
} from "./feedStoreHandlers";

// --- Types ---

export type FeedCategory = "economy" | "cards" | "event" | "system";

export interface DeltaItem {
  label: string;
  value: number;
  icon?: string;
  mode?: "delta" | "snapshot";
}

export interface CardThumbnail {
  grp_id: number;
  name: string;
  set_code: string;
  collector_number: string;
  rarity: string;
  is_new?: boolean;
  gems_compensation?: number;
  vault_compensation?: number;
}

export interface FeedEntry {
  id: number;
  timestamp: number;
  category: FeedCategory;
  kind: "entry" | "session_end";
  icon?: string;
  title: string;
  details?: string;
  deltas?: DeltaItem[];
  thumbnails?: CardThumbnail[];
  sessionId?: string;
  deckData?: DeckUpdateDetails;
}

// --- Constants ---

const MAX_ENTRIES = 200;
const STARTUP_SUPPRESSION_MS = 5000;
const LS_LAST_INVENTORY = "mtga-hub:last-session-inventory";

let nextId = 1;

// --- Signals ---

const [feedEntries, setFeedEntries] = createSignal<FeedEntry[]>([]);
const [activeFilters, setActiveFilters] = createSignal<Set<FeedCategory>>(
  new Set()
);

// --- Card dedup: suppress memory COLLECTION_CHANGED for cards already seen in log events ---

const recentLogCardIds = new Map<number, number>(); // grpId → timestamp
const LOG_CARD_DEDUP_MS = 60_000;

function recordLogCards(grpIds: number[]): void {
  const now = Date.now();
  for (const id of grpIds) recentLogCardIds.set(id, now);
}

function isRecentLogCard(grpId: number): boolean {
  const ts = recentLogCardIds.get(grpId);
  if (!ts) return false;
  if (Date.now() - ts > LOG_CARD_DEDUP_MS) {
    recentLogCardIds.delete(grpId);
    return false;
  }
  return true;
}

// --- Infinite scroll state ---

const [isLoadingMore, setIsLoadingMore] = createSignal(false);
const [hasMoreEntries, setHasMoreEntries] = createSignal(true);

// --- Derived ---

let hasFeedEntries: Accessor<boolean>;
let filteredFeedEntries: Accessor<FeedEntry[]>;

createRoot(() => {
  hasFeedEntries = () => feedEntries().length > 0;

  filteredFeedEntries = () => {
    const filters = activeFilters();
    if (filters.size === 0) return feedEntries();
    return feedEntries().filter((e) => filters.has(e.category));
  };
});

export {
  feedEntries,
  activeFilters,
  setActiveFilters,
  hasFeedEntries,
  filteredFeedEntries,
  isLoadingMore,
  hasMoreEntries,
};

// --- Filter actions ---

export function toggleFilter(category: FeedCategory): void {
  setActiveFilters((prev) => {
    const next = new Set(prev);
    if (next.has(category)) {
      next.delete(category);
    } else {
      next.add(category);
    }
    return next;
  });
}

export function clearFilters(): void {
  setActiveFilters(new Set<FeedCategory>());
}

// --- Persistence helpers ---

function rowToFeedEntry(row: FeedEntryRow): FeedEntry {
  return {
    id: row.id,
    timestamp: row.timestamp,
    category: row.category as FeedCategory,
    kind: (row.kind === "session_end" ? "session_end" : "entry") as "entry" | "session_end",
    icon: row.icon ?? undefined,
    title: row.title,
    details: row.details ?? undefined,
    deltas: row.deltas_json ? JSON.parse(row.deltas_json) : undefined,
    thumbnails: row.thumbnails_json ? JSON.parse(row.thumbnails_json) : undefined,
    sessionId: row.session_id,
  };
}

export async function loadMoreEntries(): Promise<void> {
  if (isLoadingMore() || !hasMoreEntries()) return;
  setIsLoadingMore(true);
  try {
    const entries = feedEntries();
    const lastId = entries.length > 0
      ? Math.min(...entries.map((e) => e.id))
      : null;
    const rows = await getFeedEntries(50, lastId);
    if (rows.length === 0) {
      setHasMoreEntries(false);
    } else {
      const older = rows.map(rowToFeedEntry);
      setFeedEntries((prev) => [...prev, ...older]);
      for (const r of rows) {
        if (r.id >= nextId) nextId = r.id + 1;
      }
    }
  } catch (err) {
    console.warn("Failed to load more feed entries:", err);
  } finally {
    setIsLoadingMore(false);
  }
}

// --- Entry helpers ---

function pushEntry(
  entry: Omit<FeedEntry, "id" | "timestamp" | "kind">,
  kind: "entry" | "session_end" = "entry",
): void {
  const tempId = nextId++;
  const timestamp = Date.now();
  const sessionId = getSessionId();
  const full: FeedEntry = {
    ...entry,
    id: tempId,
    timestamp,
    kind,
    sessionId: sessionId || undefined,
  };
  setFeedEntries((prev) => [full, ...prev].slice(0, MAX_ENTRIES));

  // Fire-and-forget persist to SQLite
  insertFeedEntry({
    timestamp,
    session_id: sessionId || "unknown",
    category: entry.category,
    kind,
    icon: entry.icon ?? null,
    title: entry.title,
    details: entry.details ?? null,
    deltas_json: entry.deltas ? JSON.stringify(entry.deltas) : null,
    thumbnails_json: entry.thumbnails ? JSON.stringify(entry.thumbnails) : null,
  }).then((dbId) => {
    setFeedEntries((prev) =>
      prev.map((e) => (e.id === tempId ? { ...e, id: dbId } : e))
    );
  }).catch((err) => {
    console.warn("Feed persist failed:", err);
  });
}

// --- localStorage helpers ---

function loadJson<T>(key: string): T | null {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : null;
  } catch {
    return null;
  }
}

function saveJson(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Quota exceeded — silently ignore
  }
}

// --- Lifecycle ---

const unlisteners: UnlistenFn[] = [];

export async function initFeedStore(): Promise<void> {
  // Wire up handler dependencies
  initHandlers({ pushEntry, isRecentLogCard, loadJson, saveJson });

  // Load persisted entries from SQLite before subscribing to events
  try {
    const rows = await getFeedEntries(200, null);
    if (rows.length > 0) {
      const loaded = rows.map(rowToFeedEntry);
      setFeedEntries(loaded);
      for (const r of rows) {
        if (r.id >= nextId) nextId = r.id + 1;
      }
      if (rows.length < 200) setHasMoreEntries(false);
    } else {
      setHasMoreEntries(false);
    }
  } catch (err) {
    console.warn("Failed to load persisted feed entries:", err);
  }

  unlisteners.push(
    // --- Economy / card events (pass through always) ---

    await subscribe<InventoryUpdatedPayload>(
      Events.log.INVENTORY_UPDATED,
      (payload) => {
        for (const change of payload.changes) {
          recordLogCards(change.granted_cards.map((c) => c.grp_id));
          if (change.gold_delta) accumulateDelta("gold", change.gold_delta);
          if (change.gems_delta) accumulateDelta("gems", change.gems_delta);
          if (change.granted_cards.length > 0) accumulateDelta("cards", change.granted_cards.length);
          const boosterCount = change.boosters.reduce((sum, b) => sum + b.count, 0);
          if (boosterCount > 0) accumulateDelta("packs", boosterCount);
          buildInventoryEntry(change, pushEntry);
        }
        // After recordLogCards — check if pending collection change is now fully dedup'd
        tryEarlyFlushCollection();
      }
    ),

    // CARDS_GRANTED is NOT subscribed here — the Rust backend emits it alongside
    // INVENTORY_UPDATED for the same cards, so buildInventoryEntry already covers
    // card grants. Subscribing to both would create duplicate feed entries.

    // --- Session started ---

    await subscribe<SessionStartedPayload>(
      Events.log.SESSION_STARTED,
      (payload) => {
        if (payload.is_refresh) {
          startPostMatchBuffer();
        } else {
          startSession();
          setStartupSuppression(STARTUP_SUPPRESSION_MS);
          const prev = loadJson<PlayerInventory>(LS_LAST_INVENTORY);
          buildSessionStartEntry(payload.inventory, prev, pushEntry);
          saveJson(LS_LAST_INVENTORY, payload.inventory);
        }
      }
    ),

    // --- Session start from LOG_READY (app launched into active MTGA session) ---

    await subscribe<LogReadyPayload>(
      Events.app.LOG_READY,
      async (payload) => {
        if (!payload.has_inventory || getSessionId()) return;
        try {
          const inv = await invoke<PlayerInventory | null>("get_inventory");
          if (!inv || getSessionId()) return; // re-check after await
          startSession();
          const prev = loadJson<PlayerInventory>(LS_LAST_INVENTORY);
          buildSessionStartEntry(inv, prev, pushEntry);
          saveJson(LS_LAST_INVENTORY, inv);
        } catch (err) {
          console.warn("Failed to fetch inventory for session start:", err);
        }
      }
    ),

    // --- Match result ---

    await subscribe<MatchResultPayload>(
      Events.log.MATCH_RESULT,
      (payload) => {
        captureMatchResult(payload.result);
      }
    ),

    // --- Match starting ---

    await subscribe<MatchStartingPayload>(
      Events.log.MATCH_STARTING,
      (payload) => {
        startMatchStartBuffer(payload.game_mode);
      }
    ),

    // --- Mastery (delta-only, post-match aware) ---

    await subscribe<MasteryUpdatedPayload>(
      Events.log.MASTERY_UPDATED,
      (payload) => {
        handleMasteryEvent(payload);
      }
    ),

    // --- Generic log events (whitelist dispatcher) ---

    await subscribe<LogEventPayload>(Events.log.LOG_EVENT, (payload) => {
      dispatchLogEvent(payload);
    }),

    // --- Memory events (pass through always) ---

    await subscribe<MemoryCollectionChangedPayload>(
      Events.memory.COLLECTION_CHANGED,
      (payload) => {
        bufferCollectionChange(payload);
      }
    ),

    await subscribe<InventoryChangedPayload>(
      Events.memory.INVENTORY_CHANGED,
      (payload) => {
        buildMemoryInventoryChangedEntry(payload, pushEntry);
      }
    ),

    // --- Achievements (discovery-first — log structure to console) ---

    await subscribe<AchievementsUpdatedPayload>(
      Events.log.ACHIEVEMENTS_UPDATED,
      (payload) => {
        if (payload.is_refresh) return;
        const ach = payload.achievements;
        let count = 0;
        if (Array.isArray(ach)) count = ach.length;
        else if (ach && typeof ach === "object") count = Object.keys(ach).length;

        pushEntry({
          category: "event",
          icon: "ObjectiveIcon_Gold.png",
          title: "Achievements updated",
          details: count > 0 ? `${count} achievement${count !== 1 ? "s" : ""}` : undefined,
        });
      }
    ),

    // --- Cosmetics (diff-based feed entries) ---

    await subscribe<CosmeticsUpdatedPayload>(
      Events.log.COSMETICS_UPDATED,
      (payload) => handleCosmeticsUpdated(payload),
    ),

    // --- Event courses (diff-based feed entries) ---

    await subscribe<EventsUpdatedPayload>(
      Events.log.EVENTS_UPDATED,
      (payload) => handleEventsUpdated(payload),
    ),

    // --- Diagnostics (warnings and errors only) ---

    await subscribe<DiagnosticPayload>(
      Events.diagnostics.LOGGED,
      (payload) => {
        if (payload.level.toLowerCase() === "info") return;
        buildDiagnosticEntry(payload, pushEntry);
      }
    ),

    // --- Session end — memory service detected MTGA exit ---

    await subscribe<MemoryWatchStoppedPayload>(
      Events.memory.WATCH_STOPPED,
      (payload) => {
        if (payload.reason === "MTGA.exe not running") emitSessionEnd();
      }
    ),

    // --- Session end — log watcher stopped (file watcher error, MTGA shutdown) ---

    await subscribe<WatcherStoppedPayload>(
      Events.log.WATCHER_STOPPED,
      (_payload) => {
        emitSessionEnd();
      }
    ),
  );
}

export function cleanupFeedStore(): void {
  for (const unlisten of unlisteners) {
    unlisten();
  }
  unlisteners.length = 0;
  cleanupHandlerTimers();
}
