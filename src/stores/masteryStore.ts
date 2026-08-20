// Mastery state. Tracks mastery/campaign graph progress.
// Subscribes to log:mastery_updated and hydrates from backend on startup.

import { createSignal, createMemo, createRoot } from "solid-js";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  Events,
  subscribe,
  getMastery,
  type MasteryState,
  type MasteryUpdatedPayload,
  type StartupCompletePayload,
} from "../lib/tauri";

// --- Signals ---

const [mastery, setMastery] = createSignal<MasteryState | null>(null);

// Module-scope memos need a stable root owner or Solid warns about leaks and
// never disposes them. Store lives for the app lifetime, so a root is fine.
const {
  hasMastery, nodeCount, nodesCompleted,
  milestoneCount, milestonesCompleted,
  currentLevel, maxLevel, currentXp, hasPremium, hasLevelTrack,
} = createRoot(() => ({
  hasMastery: createMemo(() => mastery() !== null),
  nodeCount: createMemo(() => mastery()?.node_count ?? 0),
  nodesCompleted: createMemo(() => mastery()?.nodes_completed ?? 0),
  milestoneCount: createMemo(() => mastery()?.milestone_count ?? 0),
  milestonesCompleted: createMemo(() => mastery()?.milestones_completed ?? 0),
  currentLevel: createMemo(() => mastery()?.current_level ?? 0),
  maxLevel: createMemo(() => mastery()?.max_level ?? 0),
  currentXp: createMemo(() => mastery()?.current_xp ?? 0),
  hasPremium: createMemo(() => mastery()?.has_premium ?? false),
  /** True when the graph contains LevelTrack_Level_N data (mastery pass, not NPE campaign) */
  hasLevelTrack: createMemo(() => (mastery()?.max_level ?? 0) > 0),
}));

export {
  mastery, hasMastery,
  nodeCount, nodesCompleted,
  milestoneCount, milestonesCompleted,
  currentLevel, maxLevel, currentXp, hasPremium, hasLevelTrack,
};

// --- Internal ---

async function refreshMastery(): Promise<void> {
  try {
    const data = await getMastery();
    if (data) setMastery(data);
  } catch {
    // Backend not ready
  }
}

// --- Lifecycle ---

const unlisteners: UnlistenFn[] = [];

export async function initMasteryStore(): Promise<void> {
  unlisteners.push(
    await subscribe<StartupCompletePayload>(
      Events.app.STARTUP_COMPLETE,
      () => refreshMastery()
    ),

    await subscribe<MasteryUpdatedPayload>(
      Events.log.MASTERY_UPDATED,
      (p) => setMastery(p.state)
    )
  );

  await refreshMastery();
}

export function cleanupMasteryStore(): void {
  for (const unlisten of unlisteners) {
    unlisten();
  }
  unlisteners.length = 0;
}
