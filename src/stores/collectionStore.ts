// Collection state. Module-level signals — same pattern as appStore.ts.
// Manages card DB cache, collection data, filters, and derived views.

import { createSignal, createMemo, createRoot, type Accessor } from "solid-js";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  Events,
  subscribe,
  getStatus,
  getAllCards,
  getCollection,
  scanCollection,
  type CardInfo,
  type CollectionScannedPayload,
  type MemoryCollectionChangedPayload,
  type MemoryWatchStartedPayload,
  type MemoryWatchStoppedPayload,
  type ScanProgressPayload,
  type CardDbLoadedPayload,
  type CardDbReadyPayload,
  type StartupCompletePayload,
  type ScanStartedPayload,
  type ScanErrorPayload,
} from "../lib/tauri";

// --- Types ---

export interface CollectionEntry {
  card: CardInfo;
  count: number;
}

export interface RarityStats {
  owned: number;
  total: number;
}

export interface SetStats {
  set_code: string;
  common: RarityStats;
  uncommon: RarityStats;
  rare: RarityStats;
  mythic: RarityStats;
  overall: RarityStats;
  percent: number;
}

export interface SetCard {
  card: CardInfo;
  owned: number;
}

export interface SetCardGroup {
  rarity: string;
  cards: SetCard[];
}

export type CompletionMode = "unique" | "playset";
export type OwnershipFilter = "all" | "owned" | "playset" | "incomplete";
export type SortField = "set" | "name" | "price" | "cmc" | "rarity" | "power" | "toughness" | "count";
export type SortDirection = "asc" | "desc";

// --- Card database ---
const [allCards, setAllCards] = createSignal<Record<number, CardInfo>>({});
let cardDbHydrated = false;

// --- Collection ---
const [collection, setCollection] = createSignal<Record<number, number>>({});
const [isScanning, setIsScanning] = createSignal(false);
const [isWatching, setIsWatching] = createSignal(false);
const [hasCollection, setHasCollection] = createSignal(false);

// --- Scan progress ---
const [scanProgress, setScanProgress] = createSignal({ current: 0, total: 0 });
const [scanStats, setScanStats] = createSignal({
  uniqueCards: 0,
  totalCopies: 0,
  scanDurationMs: 0,
});

// --- Overlay cards (cross viewer) ---
const [overlayCards, setOverlayCards] = createSignal<CardInfo[]>([]);
const [overlayInitialIndex, setOverlayInitialIndex] = createSignal(0);

/** Open the card overlay, optionally focused on a specific index. */
function openOverlay(cards: CardInfo[], initialIndex = 0): void {
  setOverlayInitialIndex(initialIndex);
  setOverlayCards(cards);
}

// --- Deck overlay ---
export interface DeckOverlayData {
  name: string;
  format?: string;
  mainboard: CardInfo[];
  sideboard: CardInfo[];
}
const [overlayDeck, setOverlayDeck] = createSignal<DeckOverlayData | null>(null);

// --- Completion mode ---
const [completionMode, setCompletionMode] = createSignal<CompletionMode>("unique");

// --- Filters ---
const [searchQuery, setSearchQuery] = createSignal("");
const [selectedSet, setSelectedSet] = createSignal("");
const [selectedRarities, setSelectedRarities] = createSignal<Set<string>>(
  new Set()
);
const [ownershipFilter, setOwnershipFilter] =
  createSignal<OwnershipFilter>("all");
const [hideLands, setHideLands] = createSignal(true);
const [sortField, setSortField] = createSignal<SortField>("set");
const [sortDirection, setSortDirection] = createSignal<SortDirection>("asc");

// --- Derived (wrapped in createRoot for module-scope disposal) ---

let availableSets: Accessor<string[]>;
let baseEntries: Accessor<CollectionEntry[]>;
let filteredCards: Accessor<CollectionEntry[]>;
let totalInCollection: Accessor<number>;
let cardsByName: Accessor<Map<string, CardInfo[]>>;
let setCompletion: Accessor<SetStats[]>;
let setCardsByRarity: Accessor<Map<string, SetCardGroup[]>>;

createRoot(() => {
  availableSets = createMemo(() => {
    const cards = allCards();
    const col = collection();
    const sets = new Set<string>();
    for (const grpIdStr of Object.keys(col)) {
      const card = cards[Number(grpIdStr)];
      if (card) sets.add(card.set_code);
    }
    return [...sets].sort();
  });

  // Pre-built entries: resolves grpId→CardInfo once when collection or card DB changes.
  // Filter memos iterate this instead of re-parsing Object.entries(col) every keystroke.
  baseEntries = createMemo(() => {
    const cards = allCards();
    const col = collection();
    const entries: CollectionEntry[] = [];
    for (const grpIdStr of Object.keys(col)) {
      const card = cards[Number(grpIdStr)];
      if (card) entries.push({ card, count: col[Number(grpIdStr)] });
    }
    return entries;
  });

  filteredCards = createMemo(() => {
    const entries = baseEntries();
    const query = searchQuery().toLowerCase();
    const set = selectedSet();
    const rarities = selectedRarities();
    const ownership = ownershipFilter();
    const noLands = hideLands();

    const result: CollectionEntry[] = [];

    for (let i = 0; i < entries.length; i++) {
      const { card, count } = entries[i];

      if (noLands && card.rarity === "basic_land") continue;
      if (query && !card.name.toLowerCase().includes(query)) continue;
      if (set && card.set_code !== set) continue;
      if (rarities.size > 0 && !rarities.has(card.rarity)) continue;
      if (ownership === "owned" && count < 1) continue;
      if (ownership === "playset" && count < 4) continue;
      if (ownership === "incomplete" && count >= 4) continue;

      result.push(entries[i]);
    }

    return result;
  });

  totalInCollection = createMemo(() => {
    return Object.keys(collection()).length;
  });

  cardsByName = createMemo(() => {
    const cards = allCards();
    const index = new Map<string, CardInfo[]>();
    for (const card of Object.values(cards)) {
      const list = index.get(card.name);
      if (list) list.push(card);
      else index.set(card.name, [card]);
    }
    return index;
  });

  const TRACKED_RARITIES = new Set(["common", "uncommon", "rare", "mythic"]);

  setCompletion = createMemo(() => {
    const cards = allCards();
    const col = collection();
    const mode = completionMode();
    const bySet = new Map<string, SetStats>();

    for (const card of Object.values(cards)) {
      if (!TRACKED_RARITIES.has(card.rarity)) continue;

      let stats = bySet.get(card.set_code);
      if (!stats) {
        stats = {
          set_code: card.set_code,
          common: { owned: 0, total: 0 },
          uncommon: { owned: 0, total: 0 },
          rare: { owned: 0, total: 0 },
          mythic: { owned: 0, total: 0 },
          overall: { owned: 0, total: 0 },
          percent: 0,
        };
        bySet.set(card.set_code, stats);
      }

      const rarityStats = stats[card.rarity as "common" | "uncommon" | "rare" | "mythic"];
      const limit = card.deck_limit > 0 ? card.deck_limit : 4;
      const qty = col[card.grp_id] ?? 0;

      if (mode === "unique") {
        rarityStats.total += 1;
        stats.overall.total += 1;
        if (qty >= 1) {
          rarityStats.owned += 1;
          stats.overall.owned += 1;
        }
      } else {
        rarityStats.total += limit;
        stats.overall.total += limit;
        const owned = Math.min(qty, limit);
        rarityStats.owned += owned;
        stats.overall.owned += owned;
      }
    }

    const result: SetStats[] = [];
    for (const stats of bySet.values()) {
      stats.percent = stats.overall.total > 0
        ? (stats.overall.owned / stats.overall.total) * 100
        : 0;
      result.push(stats);
    }

    result.sort((a, b) => b.percent - a.percent || a.set_code.localeCompare(b.set_code));
    return result;
  });

  const RARITY_ORDER = ["common", "uncommon", "rare", "mythic"];

  setCardsByRarity = createMemo(() => {
    const cards = allCards();
    const col = collection();
    const result = new Map<string, SetCardGroup[]>();

    // Group cards by set → rarity
    const bySetRarity = new Map<string, Map<string, SetCard[]>>();
    for (const card of Object.values(cards)) {
      if (!TRACKED_RARITIES.has(card.rarity)) continue;
      let setMap = bySetRarity.get(card.set_code);
      if (!setMap) {
        setMap = new Map();
        bySetRarity.set(card.set_code, setMap);
      }
      let rarityList = setMap.get(card.rarity);
      if (!rarityList) {
        rarityList = [];
        setMap.set(card.rarity, rarityList);
      }
      rarityList.push({ card, owned: col[card.grp_id] ?? 0 });
    }

    // Convert to sorted SetCardGroup arrays
    for (const [setCode, setMap] of bySetRarity) {
      const groups: SetCardGroup[] = [];
      for (const rarity of RARITY_ORDER) {
        const cards = setMap.get(rarity);
        if (cards) {
          cards.sort((a, b) => a.card.collector_number.localeCompare(b.card.collector_number, undefined, { numeric: true }));
          groups.push({ rarity, cards });
        }
      }
      result.set(setCode, groups);
    }

    return result;
  });
});

// --- Exports ---

export {
  allCards,
  collection,
  overlayCards,
  setOverlayCards,
  overlayInitialIndex,
  openOverlay,
  overlayDeck,
  setOverlayDeck,
  isScanning,
  isWatching,
  hasCollection,
  scanProgress,
  scanStats,
  searchQuery,
  setSearchQuery,
  selectedSet,
  setSelectedSet,
  selectedRarities,
  setSelectedRarities,
  ownershipFilter,
  setOwnershipFilter,
  hideLands,
  setHideLands,
  sortField,
  setSortField,
  sortDirection,
  setSortDirection,
  availableSets,
  filteredCards,
  totalInCollection,
  cardsByName,
  completionMode,
  setCompletionMode,
  setCompletion,
  setCardsByRarity,
};

// --- Actions ---

export async function doScan(): Promise<void> {
  setIsScanning(true);
  setScanProgress({ current: 0, total: 0 });
  try {
    await scanCollection(); // Returns immediately — results arrive via events
    // Don't setIsScanning(false) here — the COLLECTION_SCANNED handler does that
  } catch (e) {
    // Only fires on immediate rejection (e.g., "already scanning", "card DB not loaded")
    setIsScanning(false);
    throw e;
  }
}

// --- Lifecycle ---

const unlisteners: UnlistenFn[] = [];

export async function initCollectionStore(): Promise<void> {
  unlisteners.push(
    await subscribe<StartupCompletePayload>(
      Events.app.STARTUP_COMPLETE,
      async (p) => {
        if (p.has_collection) {
          const col = await getCollection();
          if (col) {
            setCollection(col);
            setHasCollection(true);
            const total = Object.values(col).reduce((a, b) => a + b, 0);
            setScanStats({
              uniqueCards: Object.keys(col).length,
              totalCopies: total,
              scanDurationMs: 0,
            });
          }
        }
        // Also refresh allCards in case card_db:loaded fired before listeners
        if (!cardDbHydrated) {
          try {
            const cards = await getAllCards();
            if (Object.keys(cards).length > 0) {
              setAllCards(cards);
              cardDbHydrated = true;
            }
          } catch { /* already loaded */ }
        }
        setIsScanning(false);
      }
    ),

    // Early hydration: fetch card DB as soon as it's ready (before startup_complete)
    await subscribe<CardDbReadyPayload>(
      Events.app.CARD_DB_READY,
      async () => {
        if (!cardDbHydrated) {
          try {
            const cards = await getAllCards();
            if (Object.keys(cards).length > 0) {
              setAllCards(cards);
              cardDbHydrated = true;
            }
          } catch { /* will retry via card_db:loaded */ }
        }
      }
    ),

    // Track scan lifecycle — set isScanning when backend starts scanning
    await subscribe<ScanStartedPayload>(
      Events.memory.SCAN_STARTED,
      () => {
        setIsScanning(true);
        setScanProgress({ current: 0, total: 0 });
      }
    ),

    await subscribe<CollectionScannedPayload>(
      Events.memory.COLLECTION_SCANNED,
      async (p) => {
        // Fetch the full collection from backend
        const col = await getCollection();
        if (col) {
          setCollection(col);
          setHasCollection(true);
        }
        setScanStats({
          uniqueCards: p.unique_cards,
          totalCopies: p.total_copies,
          scanDurationMs: p.scan_duration_ms,
        });
        setIsScanning(false);
      }
    ),

    // Scan error — stop scanning state
    await subscribe<ScanErrorPayload>(
      Events.memory.SCAN_ERROR,
      () => {
        setIsScanning(false);
      }
    ),

    await subscribe<MemoryCollectionChangedPayload>(
      Events.memory.COLLECTION_CHANGED,
      (p) => {
        // Apply diff to current collection
        setCollection((prev) => {
          const next = { ...prev };
          for (const [id, qty] of p.added) next[id] = qty;
          for (const [id, , newQty] of p.increased) next[id] = newQty;
          for (const [id] of p.removed) delete next[id];
          return next;
        });
      }
    ),

    await subscribe<ScanProgressPayload>(Events.memory.SCAN_PROGRESS, (p) => {
      setScanProgress({ current: p.current_region, total: p.total_regions });
    }),

    await subscribe<MemoryWatchStartedPayload>(
      Events.memory.WATCH_STARTED,
      () => {
        setIsWatching(true);
      }
    ),

    await subscribe<MemoryWatchStoppedPayload>(
      Events.memory.WATCH_STOPPED,
      () => {
        setIsWatching(false);
      }
    ),

    await subscribe<CardDbLoadedPayload>(Events.cardDb.LOADED, async () => {
      // Reload card DB when it's (re)loaded
      try {
        const cards = await getAllCards();
        if (Object.keys(cards).length > 0) {
          setAllCards(cards);
          cardDbHydrated = true;
        }
      } catch {
        // Will retry on next load
      }
    })
  );

  // Hydrate from current backend state
  if (!cardDbHydrated) {
    try {
      const cards = await getAllCards();
      if (Object.keys(cards).length > 0) {
        setAllCards(cards);
        cardDbHydrated = true;
      }
    } catch {
      // Card DB not loaded yet — will arrive via event
    }
  }

  try {
    const status = await getStatus();
    setIsWatching(status.is_watching);
    setIsScanning(status.is_scanning);
    if (status.has_collection && !hasCollection()) {
      setScanStats({
        uniqueCards: status.collection_unique,
        totalCopies: status.collection_total,
        scanDurationMs: 0,
      });
      const col = await getCollection();
      if (col) {
        setCollection(col);
        setHasCollection(true);
      }
    }
  } catch {
    // Backend not ready yet — signals will update via events
  }

}

export function cleanupCollectionStore(): void {
  for (const unlisten of unlisteners) {
    unlisten();
  }
  unlisteners.length = 0;
}
