// Economy history tab — date-range filter chips + stacked chart panels
// for Currencies, Wildcards, and Resources trends over time.

import { type Component, For, Show, createSignal, createMemo } from "solid-js";
import EconomyChart, { type ChartDataset, type YAxisConfig } from "../../components/EconomyChart";
import { economyHistory, hasHistory } from "../../stores/historyStore";
import type { EconomySnapshotRow } from "../../lib/tauri";

// --- Date range filter ---

type DateRange = "7d" | "30d" | "90d" | "all";

const RANGE_OPTIONS: { id: DateRange; label: string }[] = [
  { id: "7d", label: "7d" },
  { id: "30d", label: "30d" },
  { id: "90d", label: "90d" },
  { id: "all", label: "All" },
];

const RANGE_SECONDS: Record<DateRange, number | null> = {
  "7d": 7 * 86400,
  "30d": 30 * 86400,
  "90d": 90 * 86400,
  all: null,
};

const [dateRange, setDateRange] = createSignal<DateRange>("30d");

// --- Colors ---

const COLOR_GOLD = "#d4a843";
const COLOR_GEMS = "#4fc3f7";
const COLOR_WC_COMMON = "#b0b0b0";
const COLOR_WC_UNCOMMON = "#90c090";
const COLOR_WC_RARE = "#d4a843";
const COLOR_WC_MYTHIC = "#e06030";
const COLOR_VAULT = "#8f82ff";
const COLOR_BOOSTERS = "#7cb342";

// --- Section icons ---

const SECTION_ICONS: Record<string, string> = {
  currencies: "/icons/Nav_Coins.png",
  wildcards: "/icons/Nav_WildCard_Glow.png",
  resources: "/icons/Nav_Vault.png",
};

// --- Helpers ---

function formatDateLabel(epochSecs: number, shortRange: boolean): string {
  const d = new Date(epochSecs * 1000);
  const mon = d.toLocaleString("en-US", { month: "short" });
  const day = d.getDate();
  if (shortRange) {
    const h = d.getHours().toString().padStart(2, "0");
    const m = d.getMinutes().toString().padStart(2, "0");
    return `${mon} ${day} ${h}:${m}`;
  }
  return `${mon} ${day}`;
}

// --- Component ---

const HistoryTab: Component = () => {
  const filtered = createMemo(() => {
    const all = economyHistory();
    const rangeS = RANGE_SECONDS[dateRange()];
    if (!rangeS) return all;
    const cutoff = Date.now() / 1000 - rangeS;
    return all.filter((r) => r.timestamp >= cutoff);
  });

  const isShortRange = () => dateRange() === "7d";

  const labels = createMemo(() =>
    filtered().map((r) => formatDateLabel(r.timestamp, isShortRange()))
  );

  // --- Currency datasets ---
  const currencyDatasets = createMemo((): ChartDataset[] => {
    const rows = filtered();
    return [
      {
        label: "Gold",
        data: rows.map((r) => r.gold),
        borderColor: COLOR_GOLD,
        backgroundColor: COLOR_GOLD + "18",
        yAxisID: "y",
        fill: true,
      },
      {
        label: "Gems",
        data: rows.map((r) => r.gems),
        borderColor: COLOR_GEMS,
        backgroundColor: COLOR_GEMS + "18",
        yAxisID: "yRight",
        fill: true,
      },
    ];
  });

  const currencyYAxis: YAxisConfig = { rightAxisId: "yRight" };

  // --- Wildcard datasets ---
  const wildcardDatasets = createMemo((): ChartDataset[] => {
    const rows = filtered();
    return [
      { label: "Common", data: rows.map((r) => r.wc_common), borderColor: COLOR_WC_COMMON },
      { label: "Uncommon", data: rows.map((r) => r.wc_uncommon), borderColor: COLOR_WC_UNCOMMON },
      { label: "Rare", data: rows.map((r) => r.wc_rare), borderColor: COLOR_WC_RARE },
      { label: "Mythic", data: rows.map((r) => r.wc_mythic), borderColor: COLOR_WC_MYTHIC },
    ];
  });

  // --- Resource datasets ---
  const resourceDatasets = createMemo((): ChartDataset[] => {
    const rows = filtered();
    return [
      {
        label: "Vault %",
        data: rows.map((r) => r.vault_progress),
        borderColor: COLOR_VAULT,
        backgroundColor: COLOR_VAULT + "18",
        yAxisID: "y",
        fill: true,
      },
      {
        label: "Boosters",
        data: rows.map((r) => r.total_boosters),
        borderColor: COLOR_BOOSTERS,
        yAxisID: "yRight",
      },
    ];
  });

  const resourceYAxis: YAxisConfig = { rightAxisId: "yRight" };

  return (
    <Show when={hasHistory()} fallback={<EmptyState />}>
      <RangeBar />

      <Show when={filtered().length > 0} fallback={<NoDataInRange />}>
        <div class="history-chart-panel">
          <h3 class="economy-section-title">
            <img class="economy-section-icon" src={SECTION_ICONS.currencies} alt="" />
            Currencies
          </h3>
          <EconomyChart datasets={currencyDatasets()} labels={labels()} yAxisConfig={currencyYAxis} />
        </div>

        <div class="history-chart-panel">
          <h3 class="economy-section-title">
            <img class="economy-section-icon" src={SECTION_ICONS.wildcards} alt="" />
            Wildcards
          </h3>
          <EconomyChart datasets={wildcardDatasets()} labels={labels()} />
        </div>

        <div class="history-chart-panel">
          <h3 class="economy-section-title">
            <img class="economy-section-icon" src={SECTION_ICONS.resources} alt="" />
            Resources
          </h3>
          <EconomyChart datasets={resourceDatasets()} labels={labels()} yAxisConfig={resourceYAxis} />
        </div>
      </Show>
    </Show>
  );
};

// --- Sub-components ---

const RangeBar: Component = () => (
  <div class="history-range-bar">
    <For each={RANGE_OPTIONS}>
      {(opt) => (
        <button
          class={`feed-chip ${dateRange() === opt.id ? "active" : ""}`}
          onClick={() => setDateRange(opt.id)}
        >
          {opt.label}
        </button>
      )}
    </For>
  </div>
);

const EmptyState: Component = () => (
  <div class="history-empty">
    <p class="scan-prompt-text">
      No history data yet. Economy snapshots are recorded automatically
      when a game session starts or inventory changes occur.
    </p>
  </div>
);

const NoDataInRange: Component = () => (
  <div class="history-empty">
    <RangeBar />
    <p class="scan-prompt-text">
      No snapshots in this time range. Try a wider range.
    </p>
  </div>
);

export default HistoryTab;
