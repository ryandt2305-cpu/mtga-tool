// Cosmetics state. Tracks owned cosmetics (art styles, avatars, sleeves, etc.).
// Subscribes to log:cosmetics_updated and hydrates from backend on startup.

import { createSignal, createMemo, createRoot } from "solid-js";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  Events,
  subscribe,
  getCosmetics,
  type PlayerCosmetics,
  type CosmeticsUpdatedPayload,
  type StartupCompletePayload,
} from "../lib/tauri";
import { allCards } from "./collectionStore";

// --- Types ---

export interface ResolvedArtStyle {
  grp_id: number;
  art_id: number;
  name: string;
  set_code: string;
  rarity: string;
}

export interface ArtStyleSetGroup {
  set: string;
  items: ResolvedArtStyle[];
}

// --- Signals ---

const [cosmetics, setCosmetics] = createSignal<PlayerCosmetics | null>(null);

// --- Derived: resolved art styles ---

const RARITY_ORDER: Record<string, number> = {
  mythic: 4, rare: 3, uncommon: 2, common: 1, basic_land: 0,
};

function rarityOrder(r: string): number {
  return RARITY_ORDER[r] ?? -1;
}

// Module-scope memos need a stable root owner or Solid warns about leaks and
// never disposes them. Since this store lives for the app lifetime that's fine.
const {
  hasCosmetics,
  artStyleCount,
  avatarCount,
  sleeveCount,
  petCount,
  emoteCount,
  titleCount,
  allCategories,
  resolvedArtStyles,
  artStylesBySet,
} = createRoot(() => {
  const resolved = createMemo<ResolvedArtStyle[]>(() => {
    const c = cosmetics();
    if (!c) return [];
    const cards = allCards();
    return c.art_styles
      .map((s) => {
        const card = cards[s.grp_id];
        return card
          ? { grp_id: s.grp_id, art_id: s.art_id, name: card.name, set_code: card.set_code, rarity: card.rarity }
          : { grp_id: s.grp_id, art_id: s.art_id, name: `#${s.grp_id}`, set_code: "???", rarity: "unknown" };
      })
      .sort((a, b) => rarityOrder(b.rarity) - rarityOrder(a.rarity) || a.name.localeCompare(b.name));
  });
  return {
    hasCosmetics: createMemo(() => cosmetics() !== null),
    artStyleCount: createMemo(() => cosmetics()?.art_styles.length ?? 0),
    avatarCount: createMemo(() => cosmetics()?.avatars.length ?? 0),
    sleeveCount: createMemo(() => cosmetics()?.sleeves.length ?? 0),
    petCount: createMemo(() => cosmetics()?.pets.length ?? 0),
    emoteCount: createMemo(() => cosmetics()?.emotes.length ?? 0),
    titleCount: createMemo(() => cosmetics()?.titles.length ?? 0),
    allCategories: createMemo<string[]>(() => {
      const c = cosmetics();
      if (!c) return [];
      const known = ["art_styles", "avatars", "sleeves", "pets", "emotes", "titles"];
      const otherKeys = Object.keys(c.other ?? {});
      return [...known, ...otherKeys];
    }),
    resolvedArtStyles: resolved,
    artStylesBySet: createMemo<ArtStyleSetGroup[]>(() => {
      const styles = resolved();
      const groups = new Map<string, ResolvedArtStyle[]>();
      for (const s of styles) {
        const list = groups.get(s.set_code) ?? [];
        list.push(s);
        groups.set(s.set_code, list);
      }
      return [...groups.entries()]
        .sort((a, b) => b[1].length - a[1].length)
        .map(([set, items]) => ({ set, items }));
    }),
  };
});

export {
  cosmetics,
  hasCosmetics,
  artStyleCount,
  avatarCount,
  sleeveCount,
  petCount,
  emoteCount,
  titleCount,
  allCategories,
  resolvedArtStyles,
  artStylesBySet,
};

// --- Internal ---

async function refreshCosmetics(): Promise<void> {
  try {
    const data = await getCosmetics();
    if (data) setCosmetics(data);
  } catch {
    // Backend not ready
  }
}

// --- Lifecycle ---

const unlisteners: UnlistenFn[] = [];

export async function initCosmeticsStore(): Promise<void> {
  unlisteners.push(
    await subscribe<StartupCompletePayload>(
      Events.app.STARTUP_COMPLETE,
      () => refreshCosmetics()
    ),

    await subscribe<CosmeticsUpdatedPayload>(
      Events.log.COSMETICS_UPDATED,
      (p) => setCosmetics(p.cosmetics)
    )
  );

  await refreshCosmetics();
}

export function cleanupCosmeticsStore(): void {
  for (const unlisten of unlisteners) {
    unlisten();
  }
  unlisteners.length = 0;
}
