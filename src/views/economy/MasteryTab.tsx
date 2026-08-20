// Mastery tab — level track, node completion, and milestone progress bars.
// Extracted from Economy.tsx for file size management.

import { type Component, Show, For } from "solid-js";
import {
  mastery, hasPremium, hasLevelTrack,
  nodeCount, nodesCompleted,
  milestoneCount, milestonesCompleted,
  currentLevel, maxLevel, currentXp,
} from "../../stores/masteryStore";

// --- Helpers ---

function formatNumber(n: number): string {
  return n.toLocaleString("en-US");
}

function pctString(completed: number, total: number): string {
  if (total === 0) return "0%";
  return `${Math.round((completed / total) * 100)}%`;
}

// --- Component ---

const MasteryTab: Component = () => {
  return (
    <>
      <section class="economy-section">
        <h2 class="economy-section-title">
          <img class="economy-section-icon" src="/icons/ObjectiveIcon_OrbAndCardback.png" alt="" />
          {hasLevelTrack() ? "Mastery Pass" : "Campaign Progress"}
          <Show when={hasPremium()}>
            <span class="mastery-premium-badge">Premium</span>
          </Show>
        </h2>

        {/* Level track progress — only for mastery pass graphs */}
        <Show when={hasLevelTrack()}>
          <LevelTrackBar />
          <div class="mastery-summary">
            <div class="stat-card">
              <span class="stat-card-value">
                {currentLevel() >= maxLevel() ? `${currentLevel()} (Max)` : currentLevel()}
              </span>
              <span class="stat-card-label">Level</span>
            </div>
            <div class="stat-card">
              <span class="stat-card-value">
                {currentXp() > 0 ? `${formatNumber(currentXp())} XP` : currentLevel() >= maxLevel() ? "Complete" : "—"}
              </span>
              <span class="stat-card-label">Progress to Next</span>
            </div>
            <div class="stat-card">
              <span class="stat-card-value">{maxLevel()}</span>
              <span class="stat-card-label">Max Level</span>
            </div>
          </div>
        </Show>

        {/* Node completion progress */}
        <ProgressRow
          label="Nodes"
          completed={nodesCompleted()}
          total={nodeCount()}
        />

        {/* Milestone progress */}
        <Show when={milestoneCount() > 0}>
          <ProgressRow
            label="Milestones"
            completed={milestonesCompleted()}
            total={milestoneCount()}
          />
        </Show>
      </section>
    </>
  );
};

// --- Level Track Bar ---

const LevelTrackBar: Component = () => {
  const lvl = () => currentLevel();
  const max = () => maxLevel();
  const xp = () => currentXp();

  // Cap displayed segments for readability (no more than 80 segments)
  const segmentCount = () => Math.min(max(), 80);
  const segmentsPerLevel = () => max() > 0 ? segmentCount() / max() : 0;

  // Build segment list
  const segments = () => {
    const count = segmentCount();
    const result: { status: "completed" | "current" | "empty"; xpPct: number }[] = [];
    const current = lvl();
    const total = max();

    for (let i = 0; i < count; i++) {
      // Map segment index back to level
      const levelForSeg = total > 0 ? Math.floor((i / count) * total) + 1 : i + 1;

      if (levelForSeg < current) {
        result.push({ status: "completed", xpPct: 100 });
      } else if (levelForSeg === current && current < total) {
        // XP progress: we don't know xp_required, so just show the dot as "current"
        result.push({ status: "current", xpPct: xp() > 0 ? 50 : 0 });
      } else if (current >= total) {
        result.push({ status: "completed", xpPct: 100 });
      } else {
        result.push({ status: "empty", xpPct: 0 });
      }
    }
    return result;
  };

  return (
    <div>
      <p class="mastery-level-label">
        Level {lvl()} / {max()}
      </p>
      <div class="mastery-level-bar">
        <For each={segments()}>
          {(seg) => (
            <div
              class={`mastery-level-segment ${seg.status}`}
              style={seg.status === "current" ? { "--xp-pct": `${seg.xpPct}%` } : undefined}
            />
          )}
        </For>
      </div>
    </div>
  );
};

// --- Progress Row ---

interface ProgressRowProps {
  label: string;
  completed: number;
  total: number;
}

const ProgressRow: Component<ProgressRowProps> = (props) => {
  const pct = () => props.total > 0 ? Math.min((props.completed / props.total) * 100, 100) : 0;
  const isComplete = () => props.completed >= props.total && props.total > 0;

  return (
    <div class="progress-bar-container">
      <span class="progress-bar-label">{props.label}</span>
      <div class="progress-bar">
        <div
          class={`progress-bar-fill ${isComplete() ? "complete" : ""}`}
          style={{ width: `${pct()}%` }}
        />
      </div>
      <span class="progress-bar-value">
        {props.completed} / {props.total}
      </span>
    </div>
  );
};

export default MasteryTab;
