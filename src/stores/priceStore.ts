// Price data from Scryfall. Separate store to keep collectionStore focused.
// Two-pass fetch: exact printing → oracle_id fallback for digital reprints.
// Persistent localStorage cache for instant startup on subsequent launches.
// Wildcard-based pricing for genuinely Arena-only cards with no paper equivalent.

import { createSignal, createMemo, createRoot, createEffect, on, type Accessor } from "solid-js";
import { fetch as tauriFetch } from "@tauri-apps/plugin-http";
import { allCards, collection } from "./collectionStore";
import type { CardInfo, ImageSyncEntry } from "../lib/tauri";
import { getLocalImageUrl, syncImageCache } from "./imageCacheStore";

// --- Types ---

export type Currency = "usd" | "eur" | "aud";
export type PriceSource = "paper" | "wildcard";

interface CardPriceEntry {
  usd: number | null;
  eur: number | null;
  digital: boolean;
  oracleId: string | null;
  imgNormal: string | null;
  imgSmall: string | null;
}

// --- Cache types (internal) ---

interface CachedPriceEntry {
  u: number | null;    // usd
  e: number | null;    // eur
  d: boolean;          // digital
  o: string | null;    // oracleId
  in: string | null;   // imgNormal
  is: string | null;   // imgSmall
  t: number;           // fetched timestamp (epoch ms)
}

interface PriceCache {
  v: number;           // schema version
  entries: Record<string, CachedPriceEntry>;
}

const CACHE_KEY = "mtga-price-cache";
const CACHE_VERSION = 2;
const CACHE_TTL_MS = 24 * 60 * 60 * 1000;

// --- Wildcard value constants (derived from WotC bundle pricing) ---

const WILDCARD_USD: Record<string, number> = {
  common: 0.10,
  uncommon: 0.25,
  rare: 2.50,    // $9.99 / 4 WC
  mythic: 5.00,  // $19.99 / 4 WC
};

// --- Signals ---

const [cardPrices, setCardPrices] = createSignal<Record<number, CardPriceEntry>>({});
const [pricesLoaded, setPricesLoaded] = createSignal(false);
const [pricesLoading, setPricesLoading] = createSignal(false);
const [selectedCurrency, setSelectedCurrency] = createSignal<Currency>("usd");
const [audRate, setAudRate] = createSignal<number | null>(null);

// --- Cache read/write ---

function loadCache(): Record<number, CardPriceEntry> {
  try {
    const raw = localStorage.getItem(CACHE_KEY);
    if (!raw) return {};
    const parsed: PriceCache = JSON.parse(raw);
    if (parsed.v !== CACHE_VERSION) return {};

    const result: Record<number, CardPriceEntry> = {};
    for (const [grpId, entry] of Object.entries(parsed.entries)) {
      result[Number(grpId)] = {
        usd: entry.u,
        eur: entry.e,
        digital: entry.d,
        oracleId: entry.o,
        imgNormal: entry.in,
        imgSmall: entry.is,
      };
    }
    return result;
  } catch {
    return {};
  }
}

function saveCache(prices: Record<number, CardPriceEntry>): void {
  try {
    // Preserve timestamps from existing cache for unchanged entries
    let existingEntries: Record<string, CachedPriceEntry> = {};
    try {
      const raw = localStorage.getItem(CACHE_KEY);
      if (raw) {
        const parsed: PriceCache = JSON.parse(raw);
        if (parsed.v === CACHE_VERSION) existingEntries = parsed.entries;
      }
    } catch { /* ignore */ }

    const now = Date.now();
    const entries: Record<string, CachedPriceEntry> = {};

    for (const [grpId, entry] of Object.entries(prices)) {
      const existing = existingEntries[grpId];
      const unchanged = existing &&
        existing.u === entry.usd &&
        existing.e === entry.eur &&
        existing.d === entry.digital &&
        existing.o === entry.oracleId &&
        existing.in === entry.imgNormal &&
        existing.is === entry.imgSmall;

      entries[grpId] = {
        u: entry.usd,
        e: entry.eur,
        d: entry.digital,
        o: entry.oracleId,
        in: entry.imgNormal,
        is: entry.imgSmall,
        t: unchanged ? existing.t : now,
      };
    }

    const cache: PriceCache = { v: CACHE_VERSION, entries };
    localStorage.setItem(CACHE_KEY, JSON.stringify(cache));
  } catch (err) {
    // QuotaExceededError or other storage failures — prices work in-memory
    console.warn("Failed to save price cache:", err);
  }
}

function staleGrpIds(cache: Record<number, CardPriceEntry>, ownedGrpIds: number[]): number[] {
  // Read raw cache to get timestamps
  let timestamps: Record<string, number> = {};
  try {
    const raw = localStorage.getItem(CACHE_KEY);
    if (raw) {
      const parsed: PriceCache = JSON.parse(raw);
      if (parsed.v === CACHE_VERSION) {
        for (const [grpId, entry] of Object.entries(parsed.entries)) {
          timestamps[grpId] = entry.t;
        }
      }
    }
  } catch { /* treat all as stale */ }

  const now = Date.now();
  return ownedGrpIds.filter((grpId) => {
    if (!(grpId in cache)) return true;
    const ts = timestamps[String(grpId)];
    if (!ts) return true;
    return (now - ts) > CACHE_TTL_MS;
  });
}

// --- Helpers ---

/** Get the display price for a card in the selected currency. */
function getCardPrice(grpId: number, rarity?: string): number | null {
  const entry = cardPrices()[grpId];
  const currency = selectedCurrency();

  if (entry) {
    // Has Scryfall price (paper or paper-fallback)?
    if (entry.usd !== null || entry.eur !== null) {
      if (currency === "usd") return entry.usd;
      if (currency === "eur") return entry.eur;
      // AUD = USD * rate
      const rate = audRate();
      if (entry.usd === null || rate === null) return null;
      return entry.usd * rate;
    }
  }

  // No Scryfall price — wildcard fallback if rarity provided
  if (rarity) {
    const wcUsd = WILDCARD_USD[rarity];
    if (wcUsd !== undefined) {
      if (currency === "usd") return wcUsd;
      if (currency === "eur") {
        // Rough USD→EUR conversion (no dedicated rate fetched; use 0.92 as stable approximation)
        return wcUsd * 0.92;
      }
      // AUD
      const rate = audRate();
      if (rate === null) return null;
      return wcUsd * rate;
    }
  }

  return null;
}

/** Get the price source for a card. */
function getCardPriceSource(grpId: number, rarity: string): PriceSource | null {
  if (!pricesLoaded()) return null;

  const entry = cardPrices()[grpId];
  if (entry && (entry.usd !== null || entry.eur !== null)) {
    return "paper";
  }

  if (WILDCARD_USD[rarity] !== undefined) {
    return "wildcard";
  }

  return null;
}

/** Resolve a card image URL — local disk → CDN direct → API redirect fallback. */
function getCardImageUrl(
  card: { grp_id: number; set_code: string; collector_number: string },
  version: "normal" | "small",
): string {
  // 1. Local disk cache (instant, zero network)
  const local = getLocalImageUrl(card.grp_id, version);
  if (local) return local;
  // 2. CDN URL from price cache (no redirect)
  const entry = cardPrices()[card.grp_id];
  const cached = version === "normal" ? entry?.imgNormal : entry?.imgSmall;
  if (cached) return cached;
  // 3. API redirect fallback
  const set = card.set_code.toLowerCase();
  return `https://api.scryfall.com/cards/${set}/${card.collector_number}?format=image&version=${version}`;
}

function currencySymbol(): string {
  switch (selectedCurrency()) {
    case "usd": return "$";
    case "eur": return "\u20AC";
    case "aud": return "A$";
  }
}

// --- Derived ---

let totalCollectionValue: Accessor<number>;

createRoot(() => {
  totalCollectionValue = createMemo(() => {
    const prices = cardPrices();
    const col = collection();
    const cards = allCards();
    let total = 0;
    for (const [grpIdStr, count] of Object.entries(col)) {
      const grpId = Number(grpIdStr);
      const card = cards[grpId];
      const price = card ? getCardPrice(grpId, card.rarity) : getCardPrice(grpId);
      if (price !== null) total += price * count;
    }
    return total;
  });

});

// --- Scryfall fetch ---

const SCRYFALL_BATCH_SIZE = 75;
const SCRYFALL_DELAY_MS = 500;

// Scryfall response card shape (fields we use)
interface ScryfallCard {
  set: string;
  collector_number: string;
  oracle_id?: string;
  digital?: boolean;
  prices?: {
    usd?: string | null;
    eur?: string | null;
  };
  image_uris?: {
    small?: string;
    normal?: string;
  } | null;
  card_faces?: Array<{
    image_uris?: {
      small?: string;
      normal?: string;
    } | null;
  }>;
}

/** Extract a direct CDN image URL from a Scryfall card response (handles DFCs). */
function extractImageUrl(card: ScryfallCard, version: "small" | "normal"): string | null {
  return card.image_uris?.[version]
    ?? card.card_faces?.[0]?.image_uris?.[version]
    ?? null;
}

/** Batch-fetch from Scryfall Collection API. Handles rate limiting and pagination. */
async function batchFetch(
  identifiers: Record<string, unknown>[],
): Promise<{ data: ScryfallCard[]; notFound: Record<string, unknown>[] }> {
  const allData: ScryfallCard[] = [];
  const allNotFound: Record<string, unknown>[] = [];

  for (let i = 0; i < identifiers.length; i += SCRYFALL_BATCH_SIZE) {
    const batch = identifiers.slice(i, i + SCRYFALL_BATCH_SIZE);

    try {
      const resp = await tauriFetch("https://api.scryfall.com/cards/collection", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ identifiers: batch }),
      });

      if (!resp.ok) {
        console.warn(`Scryfall batch ${i / SCRYFALL_BATCH_SIZE} failed: ${resp.status}`);
        continue;
      }

      const json = await resp.json();
      if (json.data) allData.push(...json.data);
      if (json.not_found) allNotFound.push(...json.not_found);
    } catch (err) {
      console.warn(`Scryfall batch ${i / SCRYFALL_BATCH_SIZE} error:`, err);
    }

    // Rate limit: 500ms between requests (Collection API limit is 2/s)
    if (i + SCRYFALL_BATCH_SIZE < identifiers.length) {
      await new Promise((r) => setTimeout(r, SCRYFALL_DELAY_MS));
    }
  }

  return { data: allData, notFound: allNotFound };
}

// Guards: prevent concurrent fetches from double-firing effects
let fetchInFlight = false;
let variantFetchInFlight = false;

async function fetchPrices(
  cards: Record<number, CardInfo>,
  col: Record<number, number>,
): Promise<void> {
  if (fetchInFlight) return;
  fetchInFlight = true;

  const allGrpIds = Object.keys(col).map(Number);
  if (allGrpIds.length === 0) { fetchInFlight = false; return; }

  // Cache already loaded by initPriceStore() before this effect can fire.
  const cached = loadCache();

  // Determine which cards need fetching
  const stale = staleGrpIds(cached, allGrpIds);
  if (stale.length === 0) {
    console.log(`Prices: all ${allGrpIds.length} cards served from cache`);
    setPricesLoaded(true);
    setPricesLoading(false);
    fetchInFlight = false;
    return;
  }

  console.log(`Prices: ${stale.length}/${allGrpIds.length} cards stale, fetching...`);
  setPricesLoading(true);

  // --- Pass 1: exact set+collector_number ---

  const identifiers: { set: string; collector_number: string }[] = [];
  const keyToGrpIds = new Map<string, number[]>();
  const seen = new Set<string>();

  for (const grpId of stale) {
    const card = cards[grpId];
    if (!card) continue;
    const key = `${card.set_code.toLowerCase()}:${card.collector_number}`;
    const list = keyToGrpIds.get(key) || [];
    list.push(grpId);
    keyToGrpIds.set(key, list);

    if (!seen.has(key)) {
      seen.add(key);
      identifiers.push({ set: card.set_code.toLowerCase(), collector_number: card.collector_number });
    }
  }

  const result: Record<number, CardPriceEntry> = { ...cached };
  const digitalCards = new Map<string, number[]>(); // oracleId → grpIds needing pass 2

  const pass1 = await batchFetch(identifiers);

  for (const card of pass1.data) {
    const key = `${card.set}:${card.collector_number}`;
    const ids = keyToGrpIds.get(key);
    if (!ids) continue;

    const usd = card.prices?.usd ? parseFloat(card.prices.usd) : null;
    const eur = card.prices?.eur ? parseFloat(card.prices.eur) : null;
    const digital = card.digital === true;
    const oracleId = card.oracle_id ?? null;
    const imgNormal = extractImageUrl(card, "normal");
    const imgSmall = extractImageUrl(card, "small");

    for (const grpId of ids) {
      result[grpId] = { usd, eur, digital, oracleId, imgNormal, imgSmall };
    }

    // Collect digital-only cards for pass 2
    if (digital && usd === null && oracleId) {
      const existing = digitalCards.get(oracleId) || [];
      existing.push(...ids);
      digitalCards.set(oracleId, existing);
    }
  }

  const pass1Priced = Object.keys(result).length;
  console.log(`Pass 1: ${pass1Priced} entries (${identifiers.length} unique identifiers sent)`);

  // Intermediate save for crash safety (no signal update — pass 1 is incomplete)
  saveCache(result);

  // --- Pass 2: oracle_id fallback for digital reprints ---

  if (digitalCards.size > 0) {
    console.log(`Pass 2: ${digitalCards.size} digital-only oracle_ids to resolve...`);

    const oracleIdentifiers = Array.from(digitalCards.keys()).map(
      (oracleId) => ({ oracle_id: oracleId })
    );

    const pass2 = await batchFetch(oracleIdentifiers);

    for (const card of pass2.data) {
      const oracleId = card.oracle_id;
      if (!oracleId) continue;
      const grpIds = digitalCards.get(oracleId);
      if (!grpIds) continue;

      const digital = card.digital === true;
      const usd = card.prices?.usd ? parseFloat(card.prices.usd) : null;
      const eur = card.prices?.eur ? parseFloat(card.prices.eur) : null;

      // Only update if we got a paper printing with real prices
      if (!digital && (usd !== null || eur !== null)) {
        for (const grpId of grpIds) {
          // Preserve image URLs from pass 1 (they're for the exact printing)
          const prev = result[grpId];
          result[grpId] = {
            usd, eur, digital: false, oracleId,
            imgNormal: prev?.imgNormal ?? extractImageUrl(card, "normal"),
            imgSmall: prev?.imgSmall ?? extractImageUrl(card, "small"),
          };
        }
      }
      // If still digital: true, leave entry as-is (genuinely Alchemy-only)
    }

    const resolved = Array.from(digitalCards.values()).flat().filter(
      (grpId) => result[grpId] && !result[grpId].digital
    ).length;
    console.log(`Pass 2: ${resolved}/${digitalCards.size} resolved to paper prices`);
  }

  // Final update
  setCardPrices({ ...result });
  saveCache(result);
  setPricesLoaded(true);
  setPricesLoading(false);
  fetchInFlight = false;

  // Queue background image download — only for cards we actually fetched this run
  const staleSet = new Set(stale);
  const syncEntries: ImageSyncEntry[] = [];
  for (const [grpId, entry] of Object.entries(result)) {
    if (staleSet.has(Number(grpId)) && entry.imgNormal) {
      syncEntries.push({ grp_id: Number(grpId), url: entry.imgNormal });
    }
  }
  if (syncEntries.length > 0) syncImageCache(syncEntries);
}

/** Fetch Scryfall prices for variant cards not already in the store (on-demand from overlay). */
async function fetchVariantPrices(variants: CardInfo[]): Promise<void> {
  if (variantFetchInFlight) return;

  const current = cardPrices();
  const grpIds = variants.map(v => v.grp_id);
  const stale = staleGrpIds(current, grpIds);
  if (stale.length === 0) return;

  variantFetchInFlight = true;

  try {
    const staleSet = new Set(stale);
    const needed = variants.filter(v => staleSet.has(v.grp_id));

    // --- Pass 1: exact set+collector_number ---
    const identifiers: { set: string; collector_number: string }[] = [];
    const keyToGrpIds = new Map<string, number[]>();
    const seen = new Set<string>();

    for (const card of needed) {
      const key = `${card.set_code.toLowerCase()}:${card.collector_number}`;
      const list = keyToGrpIds.get(key) || [];
      list.push(card.grp_id);
      keyToGrpIds.set(key, list);

      if (!seen.has(key)) {
        seen.add(key);
        identifiers.push({ set: card.set_code.toLowerCase(), collector_number: card.collector_number });
      }
    }

    const newEntries: Record<number, CardPriceEntry> = {};
    const digitalCards = new Map<string, number[]>();

    const pass1 = await batchFetch(identifiers);

    for (const card of pass1.data) {
      const key = `${card.set}:${card.collector_number}`;
      const ids = keyToGrpIds.get(key);
      if (!ids) continue;

      const usd = card.prices?.usd ? parseFloat(card.prices.usd) : null;
      const eur = card.prices?.eur ? parseFloat(card.prices.eur) : null;
      const digital = card.digital === true;
      const oracleId = card.oracle_id ?? null;
      const imgNormal = extractImageUrl(card, "normal");
      const imgSmall = extractImageUrl(card, "small");

      for (const grpId of ids) {
        newEntries[grpId] = { usd, eur, digital, oracleId, imgNormal, imgSmall };
      }

      if (digital && usd === null && oracleId) {
        const existing = digitalCards.get(oracleId) || [];
        existing.push(...ids);
        digitalCards.set(oracleId, existing);
      }
    }

    // --- Pass 2: oracle_id fallback for digital reprints ---
    if (digitalCards.size > 0) {
      const oracleIdentifiers = Array.from(digitalCards.keys()).map(
        (oracleId) => ({ oracle_id: oracleId })
      );

      const pass2 = await batchFetch(oracleIdentifiers);

      for (const card of pass2.data) {
        const oracleId = card.oracle_id;
        if (!oracleId) continue;
        const grpIds = digitalCards.get(oracleId);
        if (!grpIds) continue;

        const digital = card.digital === true;
        const usd = card.prices?.usd ? parseFloat(card.prices.usd) : null;
        const eur = card.prices?.eur ? parseFloat(card.prices.eur) : null;

        if (!digital && (usd !== null || eur !== null)) {
          for (const grpId of grpIds) {
            const prev = newEntries[grpId];
            newEntries[grpId] = {
              usd, eur, digital: false, oracleId,
              imgNormal: prev?.imgNormal ?? extractImageUrl(card, "normal"),
              imgSmall: prev?.imgSmall ?? extractImageUrl(card, "small"),
            };
          }
        }
      }
    }

    if (Object.keys(newEntries).length > 0) {
      setCardPrices(prev => ({ ...prev, ...newEntries }));
      saveCache(cardPrices());

      // Queue background image download for variant cards
      const syncEntries: ImageSyncEntry[] = [];
      for (const [grpId, entry] of Object.entries(newEntries)) {
        if (entry.imgNormal) {
          syncEntries.push({ grp_id: Number(grpId), url: entry.imgNormal });
        }
      }
      if (syncEntries.length > 0) syncImageCache(syncEntries);
    }
  } finally {
    variantFetchInFlight = false;
  }
}

// --- Exchange rate ---

async function fetchAudRate(): Promise<void> {
  try {
    const resp = await tauriFetch("https://open.er-api.com/v6/latest/USD");
    if (!resp.ok) return;
    const data = await resp.json();
    const rate = data?.rates?.AUD;
    if (typeof rate === "number") setAudRate(rate);
  } catch (err) {
    console.warn("Failed to fetch AUD exchange rate:", err);
  }
}

// --- Lifecycle ---

let effectDisposer: (() => void) | null = null;

export function initPriceStore(): void {
  fetchAudRate();

  // Load cache immediately for instant UI
  const cached = loadCache();
  if (Object.keys(cached).length > 0) {
    setCardPrices(cached);
    setPricesLoaded(true);
  }

  // Watch for collection + card DB both being populated, then fetch prices
  const dispose = createRoot((dispose) => {
    createEffect(
      on([allCards, collection], ([cards, col]) => {
        if (Object.keys(cards).length > 0 && Object.keys(col).length > 0) {
          fetchPrices(cards, col);
        }
      })
    );
    return dispose;
  });
  effectDisposer = dispose;
}

export function cleanupPriceStore(): void {
  effectDisposer?.();
  effectDisposer = null;
}

// --- Exports ---

export {
  cardPrices,
  pricesLoaded,
  pricesLoading,
  selectedCurrency,
  setSelectedCurrency,
  audRate,
  totalCollectionValue,
  getCardPrice,
  getCardPriceSource,
  getCardImageUrl,
  currencySymbol,
  fetchVariantPrices,
};
