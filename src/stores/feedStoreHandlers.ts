// Event handler functions extracted from feedStore.ts.
// Handles LOG_EVENT dispatch, rank/quest/mastery deltas, match buffers,
// post-match grouping, collection buffering, and session lifecycle.

import type {
  LogEventPayload,
  RankInfoDetails,
  QuestUpdateDetails,
  DeckUpdateDetails,
  EventProgressDetails,
  MasteryUpdatedPayload,
  MemoryCollectionChangedPayload,
  MatchResultPayload,
  PlayerCosmetics,
  CosmeticsUpdatedPayload,
  EventsUpdatedPayload,
  EventCourse,
} from "../lib/tauri";
import {
  buildCollectionChangedEntry,
  setLastEventName,
} from "./feedBuilders";
import {
  buildPostMatchSummary,
  buildMatchStartEntry,
  buildRankDeltaEntry,
  buildMasteryDeltaEntry,
  buildEventProgressEntry,
  buildDeckEditEntry,
  formatRank,
  parseEventName,
  type RankState,
  type MasteryState2,
} from "./feedBuildersEvent";
import type { FeedEntry, DeltaItem } from "./feedStore";

// --- Constants ---

const MATCH_START_BUFFER_MS = 500;
const POST_MATCH_BUFFER_MS = 2000;
const COLLECTION_BUFFER_MS = 2_000;

// --- Types ---

type PushEntryFn = (
  entry: Omit<FeedEntry, "id" | "timestamp" | "kind">,
  kind?: "entry" | "session_end",
) => void;

// --- Module state ---

// Previous state (for delta-only rendering)
let previousRank: RankState | null = null;
let previousMastery: MasteryState2 | null = null;
let previousQuests: QuestUpdateDetails | null = null;

// Match-start buffer
let matchStartTimer: ReturnType<typeof setTimeout> | null = null;
let matchStartMode: string | null = null;
let matchStartDeck: DeckUpdateDetails | null = null;

// Match lifecycle
type MatchResult = MatchResultPayload["result"];
let lastMatchResult: MatchResult | null = null;
let matchActive = false;

// Post-match buffer
let postMatchTimer: ReturnType<typeof setTimeout> | null = null;
let postMatchActive = false;
let postMatchResult: MatchResult | null = null;
let postMatchRankDelta: string | null = null;
let postMatchQuestDelta: string | null = null;
let postMatchMasteryDelta: string | null = null;
let postMatchDeltas: DeltaItem[] = [];

// Collection buffer (dedup race condition fix)
let pendingCollectionChange: MemoryCollectionChangedPayload | null = null;
let pendingCollectionTimer: ReturnType<typeof setTimeout> | null = null;

// Startup suppression
let masterySuppressionUntil = 0;
let rankSuppressionUntil = 0;

// Session tracking
let currentSessionId = "";
let sessionStartTime = 0;
let sessionEndFired = false;
const sessionDeltas = new Map<string, number>();

// --- Accessor for pushEntry (set by initHandlers) ---

let _pushEntry: PushEntryFn = () => {};
let _isRecentLogCard: (grpId: number) => boolean = () => false;
let _loadJson: <T>(key: string) => T | null = () => null;
let _saveJson: (key: string, value: unknown) => void = () => {};

const LS_PREV_RANK = "mtga-hub:previous-rank";
const LS_PREV_MASTERY = "mtga-hub:previous-mastery";
const LS_PREV_QUESTS = "mtga-hub:previous-quests";
const LS_PREV_COSMETICS = "mtga-hub:previous-cosmetics";

// Cosmetics previous state (for delta detection)
let previousCosmetics: PlayerCosmetics | null = null;

// Event course previous state
let previousCourses: Map<string, EventCourse> = new Map();
let coursesInitialized = false;

/** Wire up dependencies from feedStore. Called once during initFeedStore. */
export function initHandlers(deps: {
  pushEntry: PushEntryFn;
  isRecentLogCard: (grpId: number) => boolean;
  loadJson: <T>(key: string) => T | null;
  saveJson: (key: string, value: unknown) => void;
}): void {
  _pushEntry = deps.pushEntry;
  _isRecentLogCard = deps.isRecentLogCard;
  _loadJson = deps.loadJson;
  _saveJson = deps.saveJson;
  previousRank = _loadJson<RankState>(LS_PREV_RANK);
  previousMastery = _loadJson<MasteryState2>(LS_PREV_MASTERY);
  // Migration: old stored state may lack milestones_completed
  if (previousMastery && (previousMastery as unknown as Record<string, unknown>).milestones_completed === undefined) {
    previousMastery.milestones_completed = 0;
  }
  previousQuests = _loadJson<QuestUpdateDetails>(LS_PREV_QUESTS);
  previousCosmetics = _loadJson<PlayerCosmetics>(LS_PREV_COSMETICS);
}

// --- Session tracking ---

export function getSessionId(): string { return currentSessionId; }
export function isPostMatchActive(): boolean { return postMatchActive; }

export function startSession(): void {
  currentSessionId = `session-${Date.now()}`;
  sessionStartTime = Date.now();
  sessionEndFired = false;
  sessionDeltas.clear();
}

export function setStartupSuppression(durationMs: number): void {
  const until = Date.now() + durationMs;
  masterySuppressionUntil = until;
  rankSuppressionUntil = until;
}

export function accumulateDelta(label: string, value: number): void {
  sessionDeltas.set(label, (sessionDeltas.get(label) ?? 0) + value);
}

function formatDuration(ms: number): string {
  const totalMin = Math.floor(ms / 60_000);
  if (totalMin < 60) return `${totalMin}m`;
  const h = Math.floor(totalMin / 60);
  const m = totalMin % 60;
  return m > 0 ? `${h}h ${m}m` : `${h}h`;
}

export function emitSessionEnd(): void {
  if (!currentSessionId || sessionEndFired) return;
  sessionEndFired = true;
  const duration = formatDuration(Date.now() - sessionStartTime);
  const parts: string[] = [duration];
  const gold = sessionDeltas.get("gold");
  if (gold) parts.push(`${gold > 0 ? "+" : ""}${gold} gold`);
  const gems = sessionDeltas.get("gems");
  if (gems) parts.push(`${gems > 0 ? "+" : ""}${gems} gems`);
  const cards = sessionDeltas.get("cards");
  if (cards) parts.push(`${cards} card${cards !== 1 ? "s" : ""}`);
  const packs = sessionDeltas.get("packs");
  if (packs) parts.push(`${packs} pack${packs !== 1 ? "s" : ""} opened`);
  _pushEntry(
    { category: "system", title: `Session ended — ${parts.join(" · ")}` },
    "session_end",
  );
  resetSession();
}

function resetSession(): void {
  currentSessionId = "";
  sessionStartTime = 0;
  sessionEndFired = false;
  sessionDeltas.clear();
}

// --- Collection buffer ---

export function bufferCollectionChange(payload: MemoryCollectionChangedPayload): void {
  pendingCollectionChange = payload;
  if (pendingCollectionTimer) clearTimeout(pendingCollectionTimer);
  pendingCollectionTimer = setTimeout(flushPendingCollection, COLLECTION_BUFFER_MS);
}

export function flushPendingCollection(): void {
  if (pendingCollectionTimer) {
    clearTimeout(pendingCollectionTimer);
    pendingCollectionTimer = null;
  }
  if (!pendingCollectionChange) return;
  const payload = pendingCollectionChange;
  pendingCollectionChange = null;
  const filtered = {
    ...payload,
    added: payload.added.filter(([id]) => !_isRecentLogCard(id)),
    increased: payload.increased.filter(([id]) => !_isRecentLogCard(id)),
  };
  const total = filtered.added.length + filtered.increased.length + filtered.removed.length;
  if (total > 0) {
    buildCollectionChangedEntry(filtered, _pushEntry);
  }
}

export function tryEarlyFlushCollection(): void {
  if (!pendingCollectionChange) return;
  const filtered = {
    ...pendingCollectionChange,
    added: pendingCollectionChange.added.filter(([id]) => !_isRecentLogCard(id)),
    increased: pendingCollectionChange.increased.filter(([id]) => !_isRecentLogCard(id)),
  };
  const remaining = filtered.added.length + filtered.increased.length + filtered.removed.length;
  if (remaining === 0) {
    if (pendingCollectionTimer) clearTimeout(pendingCollectionTimer);
    pendingCollectionTimer = null;
    pendingCollectionChange = null;
  }
}

// --- Match-start buffer ---

export function startMatchStartBuffer(gameMode: string): void {
  if (matchStartTimer) flushMatchStart();
  matchActive = true;
  matchStartMode = gameMode;
  matchStartDeck = null;
  matchStartTimer = setTimeout(flushMatchStart, MATCH_START_BUFFER_MS);
}

function flushMatchStart(): void {
  if (matchStartTimer) {
    clearTimeout(matchStartTimer);
    matchStartTimer = null;
  }
  if (matchStartMode) {
    buildMatchStartEntry(matchStartMode, matchStartDeck ?? undefined, _pushEntry);
    matchStartMode = null;
    matchStartDeck = null as DeckUpdateDetails | null;
  }
}

// --- Post-match buffer ---

export function captureMatchResult(result: MatchResult): void {
  lastMatchResult = result;
}

export function startPostMatchBuffer(): void {
  postMatchActive = true;
  postMatchResult = lastMatchResult;
  lastMatchResult = null;
  postMatchRankDelta = null;
  postMatchQuestDelta = null;
  postMatchMasteryDelta = null;
  postMatchDeltas = [];
  if (postMatchTimer) clearTimeout(postMatchTimer);
  postMatchTimer = setTimeout(flushPostMatch, POST_MATCH_BUFFER_MS);
}

function flushPostMatch(): void {
  if (postMatchTimer) {
    clearTimeout(postMatchTimer);
    postMatchTimer = null;
  }
  postMatchActive = false;
  matchActive = false;
  buildPostMatchSummary(
    postMatchResult,
    postMatchRankDelta,
    postMatchQuestDelta,
    postMatchMasteryDelta,
    postMatchDeltas,
    _pushEntry,
    postMatchResult?.event_id,
  );
  postMatchResult = null;
}

// --- Whitelist dispatcher for LOG_EVENT ---

export function dispatchLogEvent(payload: LogEventPayload): void {
  const type = payload.event_type;
  const d = payload.details;

  if (type === "QuestUpdate" && d) {
    handleQuestEvent(d as unknown as QuestUpdateDetails);
    return;
  }

  if ((type === "RankInfo" || type === "RankProgress") && d) {
    handleRankEvent(d as unknown as RankInfoDetails);
    return;
  }

  if (type === "DeckUpdate" && d) {
    const dd = d as unknown as DeckUpdateDetails;
    if (matchStartTimer) {
      matchStartDeck = dd;
      return;
    }
    if (matchActive) return;
    buildDeckEditEntry(dd, _pushEntry);
    return;
  }

  if (type === "EventProgress" && d) {
    const ed = d as unknown as EventProgressDetails;
    if (ed.event_name.startsWith("Play_")) return;
    setLastEventName(ed.event_name);
    buildEventProgressEntry(ed, _pushEntry);
    return;
  }
}

function handleRankEvent(details: RankInfoDetails): void {
  if (Date.now() < rankSuppressionUntil) {
    previousRank = buildRankDeltaEntry(details, previousRank, () => {});
    _saveJson(LS_PREV_RANK, previousRank);
    return;
  }

  if (postMatchActive) {
    const cCurr = details.constructed;
    const cPrev = previousRank?.constructed;
    if (cCurr && cPrev && (cCurr.class !== cPrev.class || cCurr.tier !== cPrev.tier)) {
      postMatchRankDelta = `Ranked up: ${formatRank(cPrev.class, cPrev.tier)} → ${formatRank(cCurr.class, cCurr.tier)}`;
    }
    previousRank = buildRankDeltaEntry(details, previousRank, () => {});
    _saveJson(LS_PREV_RANK, previousRank);
    return;
  }

  previousRank = buildRankDeltaEntry(details, previousRank, _pushEntry);
  _saveJson(LS_PREV_RANK, previousRank);
}

function handleQuestEvent(details: QuestUpdateDetails): void {
  if (postMatchActive) {
    const quests = details.quests;
    const completed = quests.filter((q) => q.progress >= q.goal);
    if (completed.length > 0) {
      postMatchQuestDelta = completed.map((q) => `${q.title} — ${q.progress}/${q.goal}`).join(", ");
    } else if (quests.length > 0) {
      const best = quests.reduce((a, b) =>
        (a.goal > 0 && a.progress / a.goal > b.progress / b.goal) ? a : b
      , quests[0]);
      if (best && best.goal > 0) {
        postMatchQuestDelta = `${best.title} — ${best.progress}/${best.goal}`;
      }
    }
    previousQuests = details;
    _saveJson(LS_PREV_QUESTS, previousQuests);
    return;
  }

  const questLines = details.quests
    .map((q) => `${q.title} — ${q.progress}/${q.goal}`)
    .filter((s) => s.length > 0);
  const completedCount = details.quests.filter((q) => q.progress >= q.goal).length;
  const titleSuffix = completedCount > 0 ? ` — ${completedCount} complete` : "";
  _pushEntry({
    category: "event",
    icon: "ObjectiveIcon_Gold.png",
    title: `Quest progress${titleSuffix}`,
    details: questLines.join("  ·  ") || undefined,
  });
  previousQuests = details;
  _saveJson(LS_PREV_QUESTS, previousQuests);
}

export function handleMasteryEvent(payload: MasteryUpdatedPayload): void {
  const curr = payload.state;
  if (curr.current_level === 0 && curr.max_level === 0) return;

  if (Date.now() < masterySuppressionUntil) {
    previousMastery = { level: curr.current_level, xp: curr.current_xp, milestones_completed: curr.milestones_completed };
    _saveJson(LS_PREV_MASTERY, previousMastery);
    return;
  }

  if (postMatchActive) {
    if (previousMastery && curr.current_level > previousMastery.level) {
      postMatchMasteryDelta = `Level ${previousMastery.level} → ${curr.current_level}`;
    } else if (previousMastery) {
      const xpGained = curr.current_xp - previousMastery.xp;
      if (xpGained > 0) {
        postMatchMasteryDelta = `Level ${curr.current_level} · +${xpGained} XP`;
      } else {
        postMatchMasteryDelta = `Level ${curr.current_level}`;
      }
    }
    previousMastery = { level: curr.current_level, xp: curr.current_xp, milestones_completed: curr.milestones_completed };
    _saveJson(LS_PREV_MASTERY, previousMastery);
    return;
  }

  previousMastery = buildMasteryDeltaEntry(payload, previousMastery, _pushEntry);
  _saveJson(LS_PREV_MASTERY, previousMastery);
}

// --- Cosmetics diff handler ---

export function handleCosmeticsUpdated(payload: CosmeticsUpdatedPayload): void {
  const curr = payload.cosmetics;

  if (!previousCosmetics) {
    // First observation — store as baseline, no feed entry
    previousCosmetics = curr;
    _saveJson(LS_PREV_COSMETICS, curr);
    return;
  }

  const newParts: string[] = [];
  const artDiff = curr.art_styles.length - previousCosmetics.art_styles.length;
  if (artDiff > 0) newParts.push(`${artDiff} card style${artDiff !== 1 ? "s" : ""}`);
  const avatarDiff = curr.avatars.length - previousCosmetics.avatars.length;
  if (avatarDiff > 0) newParts.push(`${avatarDiff} avatar${avatarDiff !== 1 ? "s" : ""}`);
  const sleeveDiff = curr.sleeves.length - previousCosmetics.sleeves.length;
  if (sleeveDiff > 0) newParts.push(`${sleeveDiff} sleeve${sleeveDiff !== 1 ? "s" : ""}`);
  const petDiff = curr.pets.length - previousCosmetics.pets.length;
  if (petDiff > 0) newParts.push(`${petDiff} pet${petDiff !== 1 ? "s" : ""}`);
  const emoteDiff = curr.emotes.length - previousCosmetics.emotes.length;
  if (emoteDiff > 0) newParts.push(`${emoteDiff} emote${emoteDiff !== 1 ? "s" : ""}`);
  const titleDiff = curr.titles.length - previousCosmetics.titles.length;
  if (titleDiff > 0) newParts.push(`${titleDiff} title${titleDiff !== 1 ? "s" : ""}`);

  previousCosmetics = curr;
  _saveJson(LS_PREV_COSMETICS, curr);

  if (newParts.length === 0) return;

  // COSMETICS_UPDATED fires for any change including StartHook merges.
  // Suppress during post-match to avoid duping InventoryChange cosmetics.
  if (postMatchActive) return;

  _pushEntry({
    category: "economy",
    icon: "ObjectiveIcon_AvatarMKM.png",
    title: `Cosmetics unlocked — ${newParts.join(", ")}`,
  });
}

// --- Event course diff handler ---

export function handleEventsUpdated(payload: EventsUpdatedPayload): void {
  const currMap = new Map(payload.courses.map((c) => [c.course_id, c]));

  if (!coursesInitialized) {
    // First observation — store as baseline, no feed entries
    previousCourses = currMap;
    coursesInitialized = true;
    return;
  }

  // Detect new courses (joined events)
  for (const [id, course] of currMap) {
    if (!previousCourses.has(id)) {
      // Skip Play_ queues and Color Challenges — not real events
      if (course.internal_event_name.startsWith("Play_")) continue;
      if (course.internal_event_name.startsWith("ColorChallenge_")) continue;
      if (course.internal_event_name === "DualColorPrecons") continue;

      const name = parseEventName(course.internal_event_name);
      _pushEntry({
        category: "event",
        icon: "Nav_Token.png",
        title: `Entered ${name}`,
      });
    }
  }

  // Detect completed courses (event finished)
  for (const [id, course] of currMap) {
    const prev = previousCourses.get(id);
    if (prev && prev.current_module !== "Complete" && course.current_module === "Complete") {
      if (course.internal_event_name.startsWith("Play_")) continue;
      if (course.internal_event_name.startsWith("ColorChallenge_")) continue;

      const name = parseEventName(course.internal_event_name);
      const record = course.wins != null && course.losses != null
        ? ` (${course.wins}–${course.losses})`
        : "";
      _pushEntry({
        category: "event",
        icon: "Nav_Token.png",
        title: `Event complete — ${name}${record}`,
      });
    }
  }

  previousCourses = currMap;
}

// --- Cleanup ---

export function cleanupHandlerTimers(): void {
  if (matchStartTimer) clearTimeout(matchStartTimer);
  if (postMatchTimer) clearTimeout(postMatchTimer);
  if (pendingCollectionTimer) clearTimeout(pendingCollectionTimer);
}
