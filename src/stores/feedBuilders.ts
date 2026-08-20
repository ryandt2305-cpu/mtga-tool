// Feed entry builder functions — extracted from feedStore.ts.
// Pure functions that construct FeedEntry objects from event payloads.

import type {
  InventoryChange,
  GrantedCard,
  MemoryCollectionChangedPayload,
  InventoryChangedPayload,
  MemoryInventoryScalars,
  DiagnosticPayload,
} from "../lib/tauri";
import { allCards } from "./collectionStore";
import type { FeedEntry, DeltaItem, CardThumbnail } from "./feedStore";
import { parseEventName } from "./feedBuildersEvent";

// --- Constants ---

/** Human-readable labels for InventoryChangeSource / card grant sources.
 * Covers all 52 server-side values + client-side InventoryUpdateSource conversions.
 */
export const SOURCE_LABELS: Record<string, string> = {
  // --- Packs & Cards ---
  BoosterOpen: "Pack opened",
  WildCardRedemption: "Crafted",
  CompleteVault: "Vault opened",
  DuplicateCompensation: "Duplicate protection",
  BannedCardGrant: "Banned card compensation",
  RestrictedCardGrant: "Restricted card compensation",
  EventGrantCardPool: "Event card pool",

  // --- Economy / Rewards ---
  QuestReward: "Quest reward",
  DailyWins: "Daily wins",
  WeeklyWins: "Weekly wins",
  LoginGrant: "Login reward",
  IdEmpotentLoginGrant: "Login reward",
  EventReward: "Event reward",
  EventEntryReward: "Event reward",
  EntryReward: "Event reward",
  SeasonReward: "Season reward",
  RankedSeasonReward: "Season reward",
  PlayerReward: "Player reward",
  ICR: "Card reward",
  DailyReward: "Daily reward",
  WeeklyReward: "Weekly reward",

  // --- Store ---
  MercantilePurchase: "Store purchase",
  MercantileChestPurchase: "Store chest",
  MercantileBoosterPurchase: "Store pack",
  CosmeticPurchase: "Cosmetic purchase",
  PrizeWallPurchase: "Prize wall purchase",
  RedeemVoucher: "Voucher redeemed",
  CodeRedemption: "Code redeemed",

  // --- Mastery / Battle Pass ---
  BattlePassLevelUp: "Battle pass level up",
  BattlePassLevelMasteryTree: "Mastery tree reward",
  BattlePassReward: "Mastery reward",
  MasteryReward: "Mastery reward",
  CampaignGraphPayoutNode: "Mastery reward",
  CampaignGraphAutomaticPayoutNode: "Mastery reward",
  CampaignGraphPurchaseNode: "Mastery purchase",
  CampaignGraphTieredRewardNode: "Mastery track reward",
  CampaignGraphReward: "Mastery reward",
  CatalogPurchase: "Mastery purchase",
  AccumulativePayoutNode: "Cumulative reward",

  // --- Events ---
  Draft: "Draft reward",
  Sealed: "Sealed reward",
  Constructed: "Constructed event",
  EventPayEntry: "Event entry fee",
  EventRefundEntry: "Event refund",

  // --- Progression ---
  EarlyPlayerProgressionLevelUp: "New player level up",
  EarlyPlayerProgressionMasteryTree: "New player reward",
  ProgressionRewardTierAdd: "Progression reward",
  NewPlayerExperience: "New player reward",
  StarterDeckUpgrade: "Starter deck upgrade",
  IsGrantedWithDeck: "Starter deck",
  IsGrantedFromDeck: "Starter deck",
  PreconstructedDeckReward: "Preconstructed deck",
  RenewalReward: "Set rotation reward",
  Renewal: "Set rotation reward",

  // --- Misc ---
  Letter: "Inbox reward",
  CrossPlatformReward: "Cross-platform reward",
  CustomerSupportGrant: "Support grant",
  ModifyPlayerInventory: "Inventory adjustment",
  OpenChest: "Chest opened",
  MassOpenChest: "Chests opened",
  BasicLandSetUpdate: "Basic lands updated",
  Cleanup: "Cleanup",
  Unknown: "Reward",
  VaultReward: "Vault opened",
};

/** Source-specific icons — used instead of deriving icon from first delta. */
const SOURCE_ICONS: Record<string, string> = {
  BoosterOpen: "ObjectiveIcon_Pack_Generic.png",
  WildCardRedemption: "ObjectiveIcon_Wildcard_Rare.png",
  CompleteVault: "Vault_Diffuse.png",
  VaultReward: "Vault_Diffuse.png",
  QuestReward: "ObjectiveIcon_Gold.png",
  DailyWins: "ObjectiveIcon_CoinsSmall.png",
  WeeklyWins: "ObjectiveIcon_CoinsLarge.png",
  DailyReward: "ObjectiveIcon_CoinsSmall.png",
  WeeklyReward: "ObjectiveIcon_CoinsLarge.png",
  EventReward: "Nav_Token.png",
  EventEntryReward: "Nav_Token.png",
  EntryReward: "Nav_Token.png",
  EventPayEntry: "Nav_Token.png",
  EventRefundEntry: "Nav_Token.png",
  Draft: "Nav_Token.png",
  Sealed: "Nav_Token.png",
  Constructed: "Nav_Token.png",
  BattlePassReward: "ObjectiveIcon_OrbAndCardback.png",
  MasteryReward: "ObjectiveIcon_OrbAndCardback.png",
  CampaignGraphPayoutNode: "ObjectiveIcon_OrbAndCardback.png",
  CampaignGraphAutomaticPayoutNode: "ObjectiveIcon_OrbAndCardback.png",
  CampaignGraphTieredRewardNode: "ObjectiveIcon_OrbAndCardback.png",
  CampaignGraphReward: "ObjectiveIcon_OrbAndCardback.png",
  SeasonReward: "Nav_MythicBar.png",
  RankedSeasonReward: "Nav_MythicBar.png",
  LoginGrant: "ObjectiveIcon_CoinsSmall.png",
  IdEmpotentLoginGrant: "ObjectiveIcon_CoinsSmall.png",
  MercantilePurchase: "ObjectiveIcon_Gem.png",
  MercantileBoosterPurchase: "ObjectiveIcon_Gem.png",
  CodeRedemption: "ObjectiveIcon_CoinsSmall.png",
  ICR: "ObjectiveIcon_Pack_Generic.png",
};

/** Human-readable labels for known custom token keys. */
const TOKEN_LABELS: Record<string, string> = {
  DraftToken: "Draft Token",
  SealedToken: "Sealed Token",
  Token_JumpIn: "Jump In Token",
  BonusPackProgress: "Bonus Pack Progress",
};

/** Match BattlePass_*_Orb pattern → "Mastery Orb" */
function tokenLabel(key: string): string {
  if (TOKEN_LABELS[key]) return TOKEN_LABELS[key];
  if (/^BattlePass_\w+_Orb$/i.test(key)) return "Mastery Orb";
  // Fallback: split on underscores and capitalize
  return key.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
}

/** Icon for known custom tokens. */
const TOKEN_ICONS: Record<string, string> = {
  DraftToken: "Nav_Token.png",
  SealedToken: "Nav_Token.png",
  Token_JumpIn: "ObjectiveIcon_JumpInToken.png",
};

function tokenIcon(key: string): string | undefined {
  if (TOKEN_ICONS[key]) return TOKEN_ICONS[key];
  if (/^BattlePass_\w+_Orb$/i.test(key)) return "ObjectiveIcon_OrbAndCardback.png";
  return undefined;
}

/** Sources that produce noise events — suppressed from the feed entirely. */
const SUPPRESSED_SOURCES = new Set(["BasicLandSetUpdate", "Cleanup"]);

/** Sources that should show their associated event name when available. */
const EVENT_AWARE_SOURCES = new Set([
  "EventPayEntry", "EventReward", "EventEntryReward", "EntryReward",
  "EventRefundEntry", "Draft", "Sealed", "Constructed",
]);

// --- Event context (set by feedStore when EventProgress arrives) ---

let _lastEventName: string | null = null;

/** Called by feedStore when an EventProgress log event arrives. */
export function setLastEventName(name: string | null): void {
  _lastEventName = name;
}

// --- Helpers ---

const _seenUnknownSources = new Set<string>();

export function sourceLabel(source: string): string {
  const label = SOURCE_LABELS[source];
  if (label) return label;

  // Runtime discovery — log unknown sources once per session
  if (!_seenUnknownSources.has(source)) {
    _seenUnknownSources.add(source);
    console.warn(`[FEED] Unknown InventoryChangeSource: "${source}" — add to SOURCE_LABELS`);
  }
  return formatCamelCase(source);
}

function formatCamelCase(s: string): string {
  return s.replace(/([a-z])([A-Z])/g, "$1 $2");
}

export function resolveCardThumbnails(grpIds: number[]): CardThumbnail[] {
  const cards = allCards();
  const thumbs: CardThumbnail[] = [];
  for (const id of grpIds) {
    const card = cards[id];
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
  return thumbs;
}

export function resolveGrantedThumbnails(granted: GrantedCard[]): CardThumbnail[] {
  const cards = allCards();
  const thumbs: CardThumbnail[] = [];
  for (const g of granted) {
    const card = cards[g.grp_id];
    if (card) {
      thumbs.push({
        grp_id: card.grp_id,
        name: card.name,
        set_code: card.set_code,
        collector_number: card.collector_number,
        rarity: card.rarity,
        is_new: g.card_added,
        gems_compensation: g.gems_compensation > 0 ? g.gems_compensation : undefined,
        vault_compensation: g.vault_progress > 0 ? g.vault_progress : undefined,
      });
    }
  }
  return thumbs;
}

export function cardListSummary(thumbs: CardThumbnail[], total: number): string {
  const names = thumbs.slice(0, 4).map((t) => t.name);
  if (total <= 4) return names.join(", ");
  return `${names.join(", ")} +${total - 4} more`;
}

export function cardGroupSummary(thumbs: CardThumbnail[]): string {
  const groups: Record<string, { total: number; newCount: number }> = {};
  const order = ["mythic", "rare", "uncommon", "common"];

  for (const t of thumbs) {
    const r = t.rarity.toLowerCase();
    if (!groups[r]) groups[r] = { total: 0, newCount: 0 };
    groups[r].total++;
    if (t.is_new) groups[r].newCount++;
  }

  const parts: string[] = [];
  for (const r of order) {
    const g = groups[r];
    if (!g) continue;
    let label = `${g.total} ${r}${g.total !== 1 ? "s" : ""}`;
    if (g.newCount === g.total) {
      label += " (new)";
    } else if (g.newCount > 0) {
      label += ` (${g.newCount} new)`;
    }
    parts.push(label);
  }

  // Append compensation totals
  let totalGems = 0;
  let totalVault = 0;
  for (const t of thumbs) {
    totalGems += t.gems_compensation ?? 0;
    totalVault += t.vault_compensation ?? 0;
  }
  const compParts: string[] = [];
  if (totalVault > 0) compParts.push(`+${totalVault} vault`);
  if (totalGems > 0) compParts.push(`+${totalGems} gems`);
  if (compParts.length > 0) {
    parts.push(`— ${compParts.join(", ")}`);
  }

  return parts.join(" · ");
}

// --- Builder functions ---

export type PushEntryFn = (entry: Omit<FeedEntry, "id" | "timestamp" | "kind">) => void;

export function buildInventoryEntry(change: InventoryChange, pushEntry: PushEntryFn): void {
  const source = change.source;

  // Phase 6: Suppress noise sources
  if (SUPPRESSED_SOURCES.has(source)) return;

  const deltas: DeltaItem[] = [];

  if (change.gold_delta !== 0)
    deltas.push({ label: "Gold", value: change.gold_delta, icon: "ObjectiveIcon_Gold.png" });
  if (change.gems_delta !== 0)
    deltas.push({ label: "Gems", value: change.gems_delta, icon: "ObjectiveIcon_Gem.png" });
  if (change.wc_common_delta !== 0)
    deltas.push({ label: "Common WC", value: change.wc_common_delta, icon: "Nav_WildCard_Common.png" });
  if (change.wc_uncommon_delta !== 0)
    deltas.push({ label: "Uncommon WC", value: change.wc_uncommon_delta, icon: "Nav_WildCard_Uncommon.png" });
  if (change.wc_rare_delta !== 0)
    deltas.push({ label: "Rare WC", value: change.wc_rare_delta, icon: "Nav_WildCard_Rare.png" });
  if (change.wc_mythic_delta !== 0)
    deltas.push({ label: "Mythic WC", value: change.wc_mythic_delta, icon: "Nav_WildCard_MythicRare.png" });
  if (change.vault_progress_delta !== 0)
    deltas.push({ label: "Vault", value: change.vault_progress_delta });
  if (change.xp_delta !== 0)
    deltas.push({ label: "XP", value: change.xp_delta, icon: "ObjectiveIcon_OrbAndCardback.png" });
  if (change.wc_track_position_delta !== 0)
    deltas.push({ label: "WC Track", value: change.wc_track_position_delta });

  for (const b of change.boosters) {
    if (b.count > 0) {
      const setCode = b.set_code ?? "???";
      deltas.push({
        label: `${setCode} Pack`,
        value: b.count,
        icon: `BoosterPack_${setCode.toUpperCase()}.png`,
      });
    }
  }

  // Custom token deltas (mastery orbs, jump-in tokens, draft tokens, etc.)
  for (const [key, value] of Object.entries(change.custom_tokens_delta)) {
    if (value === 0) continue;
    deltas.push({
      label: tokenLabel(key),
      value,
      icon: tokenIcon(key),
    });
  }

  // Cosmetics — check BEFORE early return so cosmetic-only changes aren't dropped
  const cosm = change.cosmetics;
  const cosmParts: string[] = [];
  if (cosm.art_styles.length > 0) cosmParts.push(`${cosm.art_styles.length} art style${cosm.art_styles.length !== 1 ? "s" : ""}`);
  if (cosm.avatars.length > 0) cosmParts.push(`${cosm.avatars.length} avatar${cosm.avatars.length !== 1 ? "s" : ""}`);
  if (cosm.sleeves.length > 0) cosmParts.push(`${cosm.sleeves.length} sleeve${cosm.sleeves.length !== 1 ? "s" : ""}`);
  if (cosm.pets.length > 0) cosmParts.push(`${cosm.pets.length} pet${cosm.pets.length !== 1 ? "s" : ""}`);
  if (cosm.emotes.length > 0) cosmParts.push(`${cosm.emotes.length} emote${cosm.emotes.length !== 1 ? "s" : ""}`);
  if (cosm.titles.length > 0) cosmParts.push(`${cosm.titles.length} title${cosm.titles.length !== 1 ? "s" : ""}`);

  if (deltas.length === 0 && change.granted_cards.length === 0 && cosmParts.length === 0) return;

  const thumbnails = resolveGrantedThumbnails(change.granted_cards);

  // Resolve event name for event-related sources (Rust-provided > frontend fallback)
  const eventName = EVENT_AWARE_SOURCES.has(source)
    ? (change.event_name ?? _lastEventName ?? null)
    : null;
  const eventLabel = eventName ? parseEventName(eventName) : null;

  // Build label — event-aware sources get "Source — EventName" format
  let label: string;
  let packSetCode: string | null = null;
  if (source === "BoosterOpen" && change.granted_cards.length > 0) {
    // Pack opening — derive set code from granted cards
    const firstSetCode = change.granted_cards[0].set_code;
    if (firstSetCode) {
      packSetCode = firstSetCode.toUpperCase();
      label = `Opened ${packSetCode} pack`;
    } else {
      label = sourceLabel(source);
    }
  } else if (eventLabel) {
    // Event-aware source with known event name
    const base = sourceLabel(source);
    label = `${base} — ${eventLabel}`;
  } else {
    label = sourceLabel(source);
  }

  const cardCount = change.granted_cards.length;
  let title: string;
  let details: string | undefined;

  if (cardCount > 0 && thumbnails.length > 0) {
    if (cardCount === 1) {
      const t = thumbnails[0];
      const copyInfo = t.is_new ? "new to collection" : undefined;
      const compInfo: string[] = [];
      if (t.gems_compensation) compInfo.push(`+${t.gems_compensation} gems`);
      if (t.vault_compensation) compInfo.push(`+${t.vault_compensation} vault`);
      const suffix = [copyInfo, ...compInfo].filter(Boolean).join(", ");
      title = `${label} — ${t.name}`;
      details = suffix ? `${t.name} (${t.rarity}, ${suffix})` : `${t.name} (${t.rarity})`;
    } else {
      title = `${label} — ${cardCount} cards`;
      details = cardGroupSummary(thumbnails);
    }
  } else if (cardCount > 0) {
    title = `${label} — ${cardCount} card${cardCount !== 1 ? "s" : ""}`;
  } else if (cosmParts.length > 0 && deltas.length === 0) {
    // Cosmetic-only change (e.g. art style from mastery, avatar unlock)
    title = `${label} — ${cosmParts.join(", ")}`;
  } else {
    title = label;
  }

  if (cosmParts.length > 0) {
    const cosmLine = `+ ${cosmParts.join(", ")}`;
    details = details ? `${details}  ·  ${cosmLine}` : cosmLine;
  }

  // Icon selection: pack-specific > source-specific > first delta icon > fallback
  const icon = packSetCode
    ? `BoosterPack_${packSetCode}.png`
    : SOURCE_ICONS[source] ?? deltas.find((d) => d.icon)?.icon ?? "ObjectiveIcon_CoinsSmall.png";
  pushEntry({ category: "economy", icon, title, details, deltas, thumbnails });
}

export function buildCollectionChangedEntry(
  payload: MemoryCollectionChangedPayload,
  pushEntry: PushEntryFn
): void {
  const addedThumbs = resolveCardThumbnails(payload.added.map(([id]) => id));
  const increasedThumbs = resolveCardThumbnails(payload.increased.map(([id]) => id));
  const removedThumbs = resolveCardThumbnails(payload.removed.map(([id]) => id));

  const totalChanges = payload.added.length + payload.increased.length + payload.removed.length;
  if (totalChanges === 0) return;

  const parts: string[] = [];
  if (addedThumbs.length > 0) {
    if (addedThumbs.length <= 3) {
      parts.push(addedThumbs.map((t) => t.name).join(", "));
    } else {
      parts.push(`${payload.added.length} new cards added`);
    }
  }
  if (payload.increased.length > 0) {
    if (increasedThumbs.length <= 3) {
      parts.push(`+1 copy: ${increasedThumbs.map((t) => t.name).join(", ")}`);
    } else {
      parts.push(`${payload.increased.length} cards gained copies`);
    }
  }
  if (payload.removed.length > 0) {
    if (removedThumbs.length <= 3) {
      parts.push(`Removed: ${removedThumbs.map((t) => t.name).join(", ")}`);
    } else {
      parts.push(`${payload.removed.length} cards removed`);
    }
  }

  const allThumbs = [...addedThumbs, ...increasedThumbs, ...removedThumbs];

  let title: string;
  if (totalChanges === 1 && allThumbs.length === 1) {
    if (payload.added.length === 1) {
      title = `Card acquired — ${allThumbs[0].name}`;
    } else if (payload.increased.length === 1) {
      const [, , newQty] = payload.increased[0];
      title = `${allThumbs[0].name} — now ${newQty} ${newQty === 1 ? "copy" : "copies"}`;
    } else {
      title = `Card removed — ${allThumbs[0].name}`;
    }
  } else {
    const actionParts: string[] = [];
    if (payload.added.length > 0) actionParts.push(`${payload.added.length} added`);
    if (payload.increased.length > 0) actionParts.push(`${payload.increased.length} upgraded`);
    if (payload.removed.length > 0) actionParts.push(`${payload.removed.length} removed`);
    title = `Collection updated — ${actionParts.join(", ")}`;
  }

  pushEntry({
    category: "cards",
    icon: "ObjectiveIcon_Pack_Generic.png",
    title,
    details: parts.join("  ·  "),
    thumbnails: allThumbs,
  });
}

export function buildMemoryInventoryChangedEntry(
  payload: InventoryChangedPayload,
  pushEntry: PushEntryFn
): void {
  const deltas: DeltaItem[] = [];
  const prev = payload.previous;
  const curr = payload.current;

  const fields: { key: keyof MemoryInventoryScalars; label: string; icon?: string }[] = [
    { key: "gold", label: "Gold", icon: "ObjectiveIcon_Gold.png" },
    { key: "gems", label: "Gems", icon: "ObjectiveIcon_Gem.png" },
    { key: "wc_common", label: "Common WC", icon: "Nav_WildCard_Common.png" },
    { key: "wc_uncommon", label: "Uncommon WC", icon: "Nav_WildCard_Uncommon.png" },
    { key: "wc_rare", label: "Rare WC", icon: "Nav_WildCard_Rare.png" },
    { key: "wc_mythic", label: "Mythic WC", icon: "Nav_WildCard_MythicRare.png" },
    { key: "vault_progress", label: "Vault" },
  ];

  for (const f of fields) {
    const delta = curr[f.key] - prev[f.key];
    if (delta !== 0) {
      deltas.push({ label: f.label, value: delta, icon: f.icon });
    }
  }

  if (deltas.length === 0) return;

  const icon = deltas.find((d) => d.icon)?.icon ?? "Nav_Coins.png";
  pushEntry({ category: "economy", icon, title: "Resources updated", deltas });
}

export function buildDiagnosticEntry(payload: DiagnosticPayload, pushEntry: PushEntryFn): void {
  const level = payload.level.toLowerCase();
  pushEntry({
    category: "system",
    icon: "Icon_Default.png",
    title: payload.message,
    details: level !== "info" ? `${payload.level} · ${payload.code}` : payload.code,
  });
}
