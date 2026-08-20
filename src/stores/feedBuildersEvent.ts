// Event-specific feed entry builders — split from feedBuilders.ts.
// Handles rank, mastery, events, matches, sessions, and post-match summaries.

import type {
  RankInfoDetails,
  DeckUpdateDetails,
  DeckCard,
  EventProgressDetails,
  MasteryUpdatedPayload,
  PlayerInventory,
} from "../lib/tauri";
import { allCards } from "./collectionStore";
import type { FeedEntry, DeltaItem, CardThumbnail } from "./feedStore";

export type PushEntryFn = (entry: Omit<FeedEntry, "id" | "timestamp" | "kind">) => void;

/** Extract a human-readable event name from InternalEventName.
 *  e.g. "QuickDraft_FDN_20250601" → "Quick Draft — FDN"
 *       "CompDraft_DSK_20241001" → "Comp Draft — DSK"
 *       "Sealed_BLB_20240730"    → "Sealed — BLB"
 */
export function parseEventName(raw: string): string {
  if (!raw) return "Unknown Event";

  const patterns: [RegExp, (m: RegExpMatchArray) => string][] = [
    [/^QuickDraft_([A-Z0-9]{3})/, (m) => `Quick Draft \u2014 ${m[1]}`],
    [/^CompDraft_([A-Z0-9]{3})/, (m) => `Comp Draft \u2014 ${m[1]}`],
    [/^PremierDraft_([A-Z0-9]{3})/, (m) => `Premier Draft \u2014 ${m[1]}`],
    [/^Premier_([A-Z0-9]{3})/, (m) => `Premier \u2014 ${m[1]}`],
    [/^TradDraft_([A-Z0-9]{3})/, (m) => `Trad Draft \u2014 ${m[1]}`],
    [/^Sealed_([A-Z0-9]{3})/, (m) => `Sealed \u2014 ${m[1]}`],
    [/^TradSealed_([A-Z0-9]{3})/, (m) => `Trad Sealed \u2014 ${m[1]}`],
    [/^MidweekMagic/, () => "Midweek Magic"],
    [/^ConstructedEvent/, () => "Constructed Event"],
    [/^TraditionalConstructed/, () => "Traditional Constructed"],
  ];

  for (const [re, fmt] of patterns) {
    const m = raw.match(re);
    if (m) return fmt(m);
  }

  // Fallback: split on underscores, capitalize, drop trailing date segments
  return raw
    .split("_")
    .filter((s) => !/^\d{8,}$/.test(s))
    .map((s) => s.charAt(0).toUpperCase() + s.slice(1))
    .join(" ");
}

export function formatRank(className: string, tier: number): string {
  if (className === "Mythic") return "Mythic";
  return `${className} Tier ${tier}`;
}

// --- Delta-only rank builder ---

export interface RankState {
  constructed: { class: string; tier: number; step: number; won: number; lost: number } | null;
  limited: { class: string; tier: number; step: number; won: number; lost: number } | null;
}

export function buildRankDeltaEntry(
  details: RankInfoDetails,
  previous: RankState | null,
  pushEntry: PushEntryFn,
): RankState {
  const current: RankState = {
    constructed: details.constructed
      ? { class: details.constructed.class, tier: details.constructed.tier, step: details.constructed.step, won: details.constructed.won ?? 0, lost: details.constructed.lost ?? 0 }
      : null,
    limited: details.limited && details.limited.tier != null
      ? { class: details.limited.class, tier: details.limited.tier, step: details.limited.step, won: details.limited.won ?? 0, lost: details.limited.lost ?? 0 }
      : null,
  };

  // If no previous state, store and show current as baseline
  if (!previous) {
    const detailParts: string[] = [];
    if (current.constructed) detailParts.push(`Constructed: ${formatRank(current.constructed.class, current.constructed.tier)}`);
    if (current.limited) detailParts.push(`Limited: ${formatRank(current.limited.class, current.limited.tier)}`);
    pushEntry({
      category: "event",
      icon: "Nav_MythicBar.png",
      title: `Rank — ${current.constructed?.class ?? current.limited?.class ?? "Unknown"}`,
      details: detailParts.join("  ·  ") || undefined,
    });
    return current;
  }

  // Compare constructed
  const cPrev = previous.constructed;
  const cCurr = current.constructed;
  const rankChanges: string[] = [];

  if (cCurr && cPrev) {
    if (cCurr.class !== cPrev.class) {
      const prevIdx = ["Beginner", "Bronze", "Silver", "Gold", "Platinum", "Diamond", "Mythic"].indexOf(cPrev.class);
      const currIdx = ["Beginner", "Bronze", "Silver", "Gold", "Platinum", "Diamond", "Mythic"].indexOf(cCurr.class);
      if (currIdx > prevIdx) {
        rankChanges.push(`Promoted to ${cCurr.class}!`);
      } else {
        rankChanges.push(`Rank reset — ${cCurr.class}`);
      }
    } else if (cCurr.tier !== cPrev.tier) {
      if (cCurr.tier < cPrev.tier) {
        rankChanges.push(`Ranked up — ${formatRank(cCurr.class, cPrev.tier)} → Tier ${cCurr.tier}`);
      } else {
        rankChanges.push(`Ranked down — ${formatRank(cCurr.class, cPrev.tier)} → Tier ${cCurr.tier}`);
      }
    } else if (cCurr.step !== cPrev.step) {
      if (cCurr.step > cPrev.step) {
        rankChanges.push(`Step gained — ${formatRank(cCurr.class, cCurr.tier)}, Step ${cCurr.step}`);
      } else {
        rankChanges.push(`Step lost — ${formatRank(cCurr.class, cCurr.tier)}, Step ${cCurr.step}`);
      }
    }
  }

  // Compare limited
  const lPrev = previous.limited;
  const lCurr = current.limited;
  if (lCurr && lPrev) {
    if (lCurr.class !== lPrev.class || lCurr.tier !== lPrev.tier) {
      rankChanges.push(`Limited: ${formatRank(lCurr.class, lCurr.tier)}`);
    }
  }

  // No change detected — suppress
  if (rankChanges.length === 0) return current;

  const detailParts: string[] = [];
  if (cCurr) {
    let rankLine = `Constructed: ${formatRank(cCurr.class, cCurr.tier)}`;
    if (cCurr.won > 0 || cCurr.lost > 0) rankLine += ` (${cCurr.won}W–${cCurr.lost}L)`;
    detailParts.push(rankLine);
  }
  if (lCurr) {
    let rankLine = `Limited: ${formatRank(lCurr.class, lCurr.tier)}`;
    if (lCurr.won > 0 || lCurr.lost > 0) rankLine += ` (${lCurr.won}W–${lCurr.lost}L)`;
    detailParts.push(rankLine);
  }

  pushEntry({
    category: "event",
    icon: "Nav_MythicBar.png",
    title: rankChanges[0],
    details: detailParts.join("  ·  ") || undefined,
  });

  return current;
}

// --- Delta-only mastery builder ---

export interface MasteryState2 {
  level: number;
  xp: number;
  milestones_completed: number;
}

export function buildMasteryDeltaEntry(
  payload: MasteryUpdatedPayload,
  previous: MasteryState2 | null,
  pushEntry: PushEntryFn,
): MasteryState2 {
  const s = payload.state;
  const current: MasteryState2 = { level: s.current_level, xp: s.current_xp, milestones_completed: s.milestones_completed };

  // Campaign/NPE graphs have no level track — suppress entirely
  if (s.current_level === 0 && s.max_level === 0) return current;

  const xpLine = s.current_xp > 0 ? `${s.current_xp.toLocaleString()} / 1,000 XP` : "";
  const premiumTag = s.has_premium ? "Premium Pass" : "";
  const detailParts = [xpLine, premiumTag].filter(Boolean);
  const details = detailParts.length > 0 ? detailParts.join(" · ") : undefined;

  if (!previous) {
    // First observation — show as baseline, no delta
    pushEntry({
      category: "event",
      icon: "ObjectiveIcon_OrbAndCardback.png",
      title: `Mastery — Level ${s.current_level}`,
      details,
    });
    return current;
  }

  // No level change — check milestones only, then suppress
  if (current.level === previous.level) {
    if (current.milestones_completed > previous.milestones_completed) {
      const diff = current.milestones_completed - previous.milestones_completed;
      const total = s.milestone_count;
      const milestoneText = total > 0
        ? `${current.milestones_completed}/${total}`
        : `${current.milestones_completed}`;
      pushEntry({
        category: "event",
        icon: "ObjectiveIcon_OrbAndCardback.png",
        title: diff === 1
          ? `Mastery milestone unlocked (${milestoneText})`
          : `${diff} mastery milestones unlocked (${milestoneText})`,
      });
    }
    return current;
  }

  // Track/season reset — current level lower than stored previous (e.g. level 20 → level 0
  // when a new mastery track starts). Show as new baseline, not a negative delta.
  if (current.level < previous.level) {
    pushEntry({
      category: "event",
      icon: "ObjectiveIcon_OrbAndCardback.png",
      title: `Mastery — New track (Level ${s.current_level})`,
      details,
    });
    return current;
  }

  const levelDiff = current.level - previous.level;
  const title = levelDiff === 1
    ? `Mastery Level Up — Level ${current.level}`
    : `Mastery Level Up — Level ${current.level} (+${levelDiff})`;

  pushEntry({
    category: "event",
    icon: "ObjectiveIcon_OrbAndCardback.png",
    title,
    details,
  });

  // Milestone detection (independent of level change)
  if (previous && current.milestones_completed > previous.milestones_completed) {
    const diff = current.milestones_completed - previous.milestones_completed;
    const total = s.milestone_count;
    const milestoneText = total > 0
      ? `${current.milestones_completed}/${total}`
      : `${current.milestones_completed}`;
    pushEntry({
      category: "event",
      icon: "ObjectiveIcon_OrbAndCardback.png",
      title: diff === 1
        ? `Mastery milestone unlocked (${milestoneText})`
        : `${diff} mastery milestones unlocked (${milestoneText})`,
    });
  }

  return current;
}

// --- Contextual event progress builder ---

export function buildEventProgressEntry(details: EventProgressDetails, pushEntry: PushEntryFn): void {
  const wins = details.wins;
  const losses = details.losses;
  const maxWins = details.max_wins;
  const maxLosses = details.max_losses;
  const eventName = parseEventName(details.event_name);

  if (wins != null && losses != null && maxWins != null && maxLosses != null) {
    if (wins === maxWins) {
      pushEntry({
        category: "event",
        icon: "Nav_Token.png",
        title: `${eventName} — Complete (${wins}–${losses})`,
      });
      return;
    }
    if (losses === maxLosses) {
      pushEntry({
        category: "event",
        icon: "Nav_Token.png",
        title: `${eventName} — Eliminated (${wins}–${losses})`,
      });
      return;
    }
    if (losses >= maxLosses - 1) {
      pushEntry({
        category: "event",
        icon: "Nav_Token.png",
        title: `${eventName} — ${wins}–${losses} · 1 loss from elimination`,
      });
      return;
    }
    if (wins >= maxWins - 1) {
      pushEntry({
        category: "event",
        icon: "Nav_Token.png",
        title: `${eventName} — ${wins}–${losses} · 1 win from max!`,
      });
      return;
    }
    pushEntry({
      category: "event",
      icon: "Nav_Token.png",
      title: eventName,
      details: `${wins}–${losses}${details.module ? `  ·  ${details.module}` : ""}`,
    });
    return;
  }

  pushEntry({
    category: "event",
    icon: "Nav_Token.png",
    title: eventName,
    details: details.module || undefined,
  });
}

// --- Deck thumbnail resolver ---

export function resolveDeckThumbnails(cards: DeckCard[]): CardThumbnail[] {
  const db = allCards();
  const thumbs: CardThumbnail[] = [];
  const rarityOrder = ["mythic", "rare", "uncommon", "common", "basic_land", "special"];

  for (const dc of cards) {
    const card = db[dc.grp_id];
    if (card) {
      thumbs.push({
        grp_id: card.grp_id,
        name: card.name,
        set_code: card.set_code,
        collector_number: card.collector_number,
        rarity: card.rarity,
      });
    }
  }

  // Sort by rarity (mythic first)
  thumbs.sort((a, b) => {
    const ai = rarityOrder.indexOf(a.rarity.toLowerCase());
    const bi = rarityOrder.indexOf(b.rarity.toLowerCase());
    return (ai === -1 ? 99 : ai) - (bi === -1 ? 99 : bi);
  });

  return thumbs;
}

// --- Standalone deck edit builder ---

export function buildDeckEditEntry(details: DeckUpdateDetails, pushEntry: PushEntryFn): void {
  const thumbnails = details.mainboard ? resolveDeckThumbnails(details.mainboard) : undefined;
  pushEntry({
    category: "event",
    icon: "ObjectiveEventIcon_DeckBox.png",
    title: `Deck edited — ${details.name}`,
    details: details.format || undefined,
    thumbnails,
    deckData: details,
  });
}

// --- Match start builder ---

/** Format a game mode string for display.
 *  e.g. "Brawl_Historic" → "Historic Brawl", "Ranked" → "Ranked", "Bot" → "Bot Match"
 */
function formatGameMode(raw: string): string {
  const map: Record<string, string> = {
    Ranked: "Ranked",
    Bot: "Bot Match",
    Brawl: "Brawl",
    Brawl_Historic: "Historic Brawl",
    Brawl_Standard: "Standard Brawl",
    Singleton: "Singleton",
    Jump_In: "Jump In",
    Alchemy: "Alchemy",
    Historic: "Historic",
    Explorer: "Explorer",
    Timeless: "Timeless",
    Standard: "Standard",
    Momir: "Momir",
    Pauper: "Pauper",
    Artisan: "Artisan",
  };
  return map[raw] ?? raw.replace(/_/g, " ");
}

export function buildMatchStartEntry(gameMode: string, deck: DeckUpdateDetails | undefined, pushEntry: PushEntryFn): void {
  const thumbnails = deck?.mainboard ? resolveDeckThumbnails(deck.mainboard) : undefined;
  const formattedMode = formatGameMode(gameMode);
  pushEntry({
    category: "event",
    icon: "ObjectiveEventIcon_DeckBox.png",
    title: `Starting match — ${formattedMode}`,
    details: deck?.name || undefined,
    thumbnails,
    deckData: deck,
  });
}

// --- Session start inventory snapshot builder ---

export function buildSessionStartEntry(
  inventory: PlayerInventory,
  previousInventory: PlayerInventory | null,
  pushEntry: PushEntryFn,
): void {
  const deltas: DeltaItem[] = [];

  const snap = "snapshot" as const;
  deltas.push({ label: "Gold", value: inventory.gold, icon: "ObjectiveIcon_Gold.png", mode: snap });
  deltas.push({ label: "Gems", value: inventory.gems, icon: "ObjectiveIcon_Gem.png", mode: snap });
  if (inventory.wc_common > 0)
    deltas.push({ label: "Common WC", value: inventory.wc_common, icon: "Nav_WildCard_Common.png", mode: snap });
  if (inventory.wc_uncommon > 0)
    deltas.push({ label: "Uncommon WC", value: inventory.wc_uncommon, icon: "Nav_WildCard_Uncommon.png", mode: snap });
  if (inventory.wc_rare > 0)
    deltas.push({ label: "Rare WC", value: inventory.wc_rare, icon: "Nav_WildCard_Rare.png", mode: snap });
  if (inventory.wc_mythic > 0)
    deltas.push({ label: "Mythic WC", value: inventory.wc_mythic, icon: "Nav_WildCard_MythicRare.png", mode: snap });
  if (inventory.vault_progress > 0)
    deltas.push({ label: "Vault", value: inventory.vault_progress, mode: snap });
  if (inventory.draft_tokens > 0)
    deltas.push({ label: "Draft Tokens", value: inventory.draft_tokens, mode: snap });
  if (inventory.sealed_tokens > 0)
    deltas.push({ label: "Sealed Tokens", value: inventory.sealed_tokens, mode: snap });

  for (const b of inventory.boosters) {
    if (b.count > 0) {
      const setCode = b.set_code ?? "???";
      deltas.push({
        label: `${setCode} Packs`,
        value: b.count,
        icon: `BoosterPack_${setCode.toUpperCase()}.png`,
        mode: snap,
      });
    }
  }

  // Cross-session comparison
  let details: string;
  if (previousInventory) {
    const changes: string[] = [];
    const goldDiff = inventory.gold - previousInventory.gold;
    if (goldDiff !== 0) changes.push(`${goldDiff > 0 ? "+" : ""}${goldDiff.toLocaleString()} gold`);
    const gemsDiff = inventory.gems - previousInventory.gems;
    if (gemsDiff !== 0) changes.push(`${gemsDiff > 0 ? "+" : ""}${gemsDiff.toLocaleString()} gems`);
    const rDiff = inventory.wc_rare - previousInventory.wc_rare;
    if (rDiff !== 0) changes.push(`${rDiff > 0 ? "+" : ""}${rDiff} rare WC`);
    const mDiff = inventory.wc_mythic - previousInventory.wc_mythic;
    if (mDiff !== 0) changes.push(`${mDiff > 0 ? "+" : ""}${mDiff} mythic WC`);
    details = changes.length > 0 ? `Since last session: ${changes.join(", ")}` : "No changes since last session";
  } else {
    details = "First tracked session";
  }

  pushEntry({
    category: "economy",
    icon: "Nav_BetaIcon.png",
    title: "Session started",
    details,
    deltas,
  });
}

// --- Post-match summary builder ---

/** Map MatchCompletedReasonType to a user-friendly suffix (only for non-standard reasons). */
function formatCompletedReason(reason: string): string | null {
  if (!reason || reason === "MatchCompletedReasonType_Success") return null;
  const cleaned = reason
    .replace("MatchCompletedReasonType_", "")
    .replace(/([a-z])([A-Z])/g, "$1 $2");
  const map: Record<string, string> = {
    Concede: "Conceded",
    Timeout: "Timeout",
    TourneyDrop: "Dropped from event",
    ClientDisconnect: "Disconnected",
    ServerDisconnect: "Server disconnect",
  };
  return map[cleaned] ?? cleaned;
}

export function buildPostMatchSummary(
  matchResult: { opponent_name: string; result: string; completed_reason?: string } | null,
  rankDelta: string | null,
  questDelta: string | null,
  masteryDelta: string | null,
  inventoryDeltas: DeltaItem[],
  pushEntry: PushEntryFn,
  eventId?: string,
): void {
  const parts = [rankDelta, questDelta, masteryDelta].filter(Boolean);
  const hasRankChange = rankDelta != null;

  // Derive game mode tag from event_id
  let modeTag = "";
  if (eventId) {
    if (eventId.startsWith("Play_")) {
      modeTag = formatGameMode(eventId.slice(5)); // strip "Play_" prefix
    } else {
      modeTag = parseEventName(eventId);
    }
  }

  let title = "Match complete";
  if (matchResult) {
    const opponent = matchResult.opponent_name.split("#")[0];
    const reasonSuffix = matchResult.completed_reason
      ? formatCompletedReason(matchResult.completed_reason)
      : null;
    const reasonTag = reasonSuffix ? ` (${reasonSuffix})` : "";
    const modePart = modeTag ? ` (${modeTag})` : "";

    if (matchResult.result === "Victory") {
      title = `Victory${modePart} — vs ${opponent}${reasonTag}`;
    } else if (matchResult.result === "Defeat") {
      title = `Defeat${modePart} — vs ${opponent}${reasonTag}`;
    } else if (matchResult.result === "Draw") {
      title = `Draw${modePart} — vs ${opponent}${reasonTag}`;
    } else {
      title = `Match complete${modePart} — vs ${opponent}${reasonTag}`;
    }
  }

  const details = parts.length > 0 ? parts.join("  ·  ") : "No changes";
  const icon = hasRankChange ? "Nav_MythicBar.png" : "Nav_Token.png";

  pushEntry({
    category: "event",
    icon,
    title,
    details,
    deltas: inventoryDeltas.length > 0 ? inventoryDeltas : undefined,
  });
}
