// Sets completion tab — per-set, per-rarity progress with visual polish.

import { type Component, For, Show, createSignal, createMemo } from "solid-js";
import {
  setCompletion,
  completionMode,
  setCompletionMode,
  allCards,
  setCardsByRarity,
  openOverlay,
  type SetStats,
  type SetCard,
  type CompletionMode,
} from "../../stores/collectionStore";
import { scryfallSetSvg, setDisplayName } from "../../lib/scryfall";
import { getCardImageUrl, getCardPrice, getCardPriceSource, currencySymbol, pricesLoaded } from "../../stores/priceStore";
import Tooltip from "../../components/Tooltip";

// --- Accordion state ---

const [expandedSet, setExpandedSet] = createSignal<string | null>(null);

function toggleExpand(setCode: string) {
  setExpandedSet((prev) => (prev === setCode ? null : setCode));
}

// --- Helpers ---

function pctStr(owned: number, total: number): string {
  if (total === 0) return "0%";
  return ((owned / total) * 100).toFixed(0) + "%";
}

function pctNum(owned: number, total: number): number {
  if (total === 0) return 0;
  return (owned / total) * 100;
}

function completionTintColor(percent: number): string {
  if (percent >= 100) return "var(--color-status-ok)";   // green #4ade80
  if (percent >= 67) return "#a3e635";                    // lime
  if (percent >= 34) return "var(--color-status-warn)";   // amber #fbbf24
  return "var(--color-status-error)";                     // red #f87171
}

function rarityClass(rarity: string): string {
  switch (rarity) {
    case "mythic": return "rarity-mythic";
    case "rare": return "rarity-rare";
    case "uncommon": return "rarity-uncommon";
    default: return "";
  }
}

const RARITY_COLORS: Record<string, string> = {
  common: "#aaa",
  uncommon: "#7bc67e",
  rare: "#e6b84d",
  mythic: "#e07c4a",
};

const WILDCARD_ICONS: Record<string, string> = {
  common: "/icons/Nav_WildCard_Common.png",
  uncommon: "/icons/Nav_WildCard_Uncommon.png",
  rare: "/icons/Nav_WildCard_Rare.png",
  mythic: "/icons/Nav_WildCard_MythicRare.png",
};

const RARITY_LABELS: { key: "common" | "uncommon" | "rare" | "mythic"; label: string }[] = [
  { key: "common", label: "Common" },
  { key: "uncommon", label: "Uncommon" },
  { key: "rare", label: "Rare" },
  { key: "mythic", label: "Mythic" },
];

// --- Per-set value computation ---

interface SetValue { owned: number; full: number }

function computeSetValue(setCode: string): SetValue {
  const groups = setCardsByRarity().get(setCode);
  if (!groups) return { owned: 0, full: 0 };
  const mode = completionMode();
  let owned = 0;
  let full = 0;
  for (const group of groups) {
    for (const sc of group.cards) {
      const price = getCardPrice(sc.card.grp_id, sc.card.rarity);
      if (price === null) continue;
      const limit = sc.card.deck_limit > 0 ? sc.card.deck_limit : 4;
      const maxCopies = mode === "unique" ? 1 : limit;
      const ownedCopies = mode === "unique" ? Math.min(sc.owned, 1) : Math.min(sc.owned, limit);
      owned += price * ownedCopies;
      full += price * maxCopies;
    }
  }
  return { owned, full };
}

function formatValue(n: number): string {
  if (n >= 1000) return (n / 1000).toFixed(1).replace(/\.0$/, "") + "k";
  return n.toFixed(0);
}

// --- Card thumbnail ---

const CardThumb: Component<{ sc: SetCard; onClick: () => void }> = (props) => {
  const [state, setState] = createSignal<"loading" | "loaded" | "error">("loading");
  const isOwned = () => props.sc.owned > 0;
  const price = createMemo(() => getCardPrice(props.sc.card.grp_id, props.sc.card.rarity));
  const priceSource = createMemo(() => getCardPriceSource(props.sc.card.grp_id, props.sc.card.rarity));

  return (
    <Tooltip text={`${props.sc.card.name} (×${props.sc.owned})`} contents>
    <div
      class={`sets-card-thumb ${isOwned() ? "" : "unowned"} ${rarityClass(props.sc.card.rarity)}`}
      onClick={() => props.onClick()}
    >
      <Show when={state() === "loading"}>
        <div class="sets-card-thumb-shimmer" />
      </Show>
      <Show when={state() !== "error"}>
        <img
          src={getCardImageUrl(props.sc.card, "small")}
          alt={props.sc.card.name}
          style={{ display: state() === "loaded" ? "block" : "none" }}
          onLoad={() => setState("loaded")}
          onError={() => setState("error")}
        />
      </Show>
      <Show when={state() === "error"}>
        <div class="sets-card-thumb-fallback">{props.sc.card.name}</div>
      </Show>
      {/* Price badge — compact, reuses card-price pattern */}
      <Show when={price() !== null && priceSource() === "paper"}>
        <span class="sets-card-thumb-price">{currencySymbol()}{price()!.toFixed(2)}</span>
      </Show>
      <Show when={price() !== null && priceSource() === "wildcard"}>
        <span class="sets-card-thumb-price sets-card-thumb-price-wc">
          <img src={WILDCARD_ICONS[props.sc.card.rarity]} alt="" />
          {currencySymbol()}{price()!.toFixed(2)}
        </span>
      </Show>
      <Show when={price() === null && pricesLoaded()}>
        <span class="sets-card-thumb-price sets-card-thumb-price-na">N/A</span>
      </Show>
      {/* Count badge */}
      <span class="sets-card-thumb-count">×{props.sc.owned}</span>
    </div>
    </Tooltip>
  );
};

// --- Card grid with rarity filters ---

const CardGrid: Component<{ setCode: string }> = (props) => {
  const [activeRarities, setActiveRarities] = createSignal<Set<string>>(new Set(["rare", "mythic"]));

  function toggleRarity(rarity: string) {
    setActiveRarities((prev) => {
      const next = new Set(prev);
      if (next.has(rarity)) next.delete(rarity);
      else next.add(rarity);
      return next;
    });
  }

  const filteredCards = createMemo(() => {
    const groups = setCardsByRarity().get(props.setCode);
    if (!groups) return [];
    const active = activeRarities();
    const result: SetCard[] = [];
    for (const group of groups) {
      if (active.has(group.rarity)) {
        result.push(...group.cards);
      }
    }
    return result;
  });

  return (
    <>
      <div class="sets-card-filters">
        <For each={RARITY_LABELS}>
          {(r) => (
            <button
              class={`feed-chip ${activeRarities().has(r.key) ? "active" : ""}`}
              onClick={() => toggleRarity(r.key)}
            >
              <img class="sets-rarity-icon" src={WILDCARD_ICONS[r.key]} alt="" />
              {r.label}
            </button>
          )}
        </For>
      </div>
      <Show when={filteredCards().length > 0}>
        <div class="sets-card-grid">
          <For each={filteredCards()}>
            {(sc, i) => (
              <CardThumb
                sc={sc}
                onClick={() => {
                  const allCards = filteredCards().map((s) => s.card);
                  openOverlay(allCards, i());
                }}
              />
            )}
          </For>
        </div>
      </Show>
    </>
  );
};

// --- Set row ---

const SetRow: Component<{ stats: SetStats }> = (props) => {
  const isExpanded = () => expandedSet() === props.stats.set_code;
  const value = createMemo(() => computeSetValue(props.stats.set_code));

  return (
    <div class="sets-row-wrapper">
      <div
        class={`sets-row ${isExpanded() ? "sets-row--expanded" : ""}`}
        style={{ "border-left-color": completionTintColor(props.stats.percent) }}
        onClick={() => toggleExpand(props.stats.set_code)}
      >
        <Tooltip text={setDisplayName(props.stats.set_code)}>
          <div class="sets-row-icon">
            <img
              src={scryfallSetSvg(props.stats.set_code)}
              alt={setDisplayName(props.stats.set_code)}
              onError={(e) => {
                (e.target as HTMLImageElement).style.display = "none";
              }}
            />
          </div>
        </Tooltip>
        <Tooltip text={setDisplayName(props.stats.set_code)}>
          <span class="sets-row-code">{props.stats.set_code}</span>
        </Tooltip>
        <div class="sets-row-bar">
          <div
            class="sets-row-bar-fill"
            style={{ width: `${Math.min(props.stats.percent, 100)}%` }}
          />
        </div>
        <span class="sets-row-pct">{props.stats.percent.toFixed(0)}%</span>
        <span class="sets-row-rarities">
          <For each={RARITY_LABELS}>
            {(r) => (
              <span class="sets-row-rarity-chip" style={{ color: RARITY_COLORS[r.key] }}>
                <img class="sets-row-rarity-wc" src={WILDCARD_ICONS[r.key]} alt={r.label} />
                {pctStr(props.stats[r.key].owned, props.stats[r.key].total)}
              </span>
            )}
          </For>
        </span>
        <span class={`sets-row-chevron ${isExpanded() ? "expanded" : ""}`}>&#x25B6;</span>
        <Show when={pricesLoaded() && value().full > 0}>
          <span class="sets-row-value">
            <span style={{ color: completionTintColor(props.stats.percent) }}>{currencySymbol()}{formatValue(value().owned)}</span>
            <span class="sets-row-value-sep">/</span>
            {currencySymbol()}{formatValue(value().full)}
          </span>
        </Show>
      </div>
      <Show when={isExpanded()}>
        <div class="sets-expanded">
          <For each={RARITY_LABELS}>
            {(r) => {
              const rs = () => props.stats[r.key];
              return (
                <div class="sets-rarity-row">
                  <img class="sets-rarity-icon" src={WILDCARD_ICONS[r.key]} alt={r.label} />
                  <span class="sets-rarity-label">{r.label}</span>
                  <div class="sets-rarity-bar">
                    <div
                      class="sets-rarity-bar-fill"
                      style={{
                        width: `${pctNum(rs().owned, rs().total)}%`,
                        background: RARITY_COLORS[r.key],
                      }}
                    />
                  </div>
                  <span class="sets-rarity-count">
                    {rs().owned} / {rs().total}
                  </span>
                </div>
              );
            }}
          </For>
          <CardGrid setCode={props.stats.set_code} />
        </div>
      </Show>
    </div>
  );
};

const SetsTab: Component = () => {
  const hasCards = () => Object.keys(allCards()).length > 0;
  const [setSearch, setSetSearch] = createSignal("");
  const modes: { id: CompletionMode; label: string }[] = [
    { id: "unique", label: "Unique" },
    { id: "playset", label: "Playset" },
  ];

  const filteredSets = createMemo(() => {
    const query = setSearch().toLowerCase().trim();
    const all = setCompletion();
    if (!query) return all;
    return all.filter((s) => {
      const code = s.set_code.toLowerCase();
      const name = setDisplayName(s.set_code).toLowerCase();
      return code.includes(query) || name.includes(query);
    });
  });

  return (
    <div>
      <Show when={hasCards()} fallback={<SetsEmpty />}>
        <div class="sets-toggle-bar">
          <For each={modes}>
            {(m) => (
              <button
                class={`feed-chip ${completionMode() === m.id ? "active" : ""}`}
                onClick={() => setCompletionMode(m.id)}
              >
                {m.label}
              </button>
            )}
          </For>
          <input
            class="sets-search"
            type="text"
            placeholder="Search sets..."
            value={setSearch()}
            onInput={(e) => setSetSearch(e.currentTarget.value)}
          />
        </div>
        <div class="sets-list">
          <For each={filteredSets()}>
            {(stats) => <SetRow stats={stats} />}
          </For>
        </div>
      </Show>
    </div>
  );
};

const SetsEmpty: Component = () => (
  <div class="sets-empty">
    <p>No card data loaded.</p>
    <p class="sets-empty-hint">Load the card database from Settings to see set completion.</p>
  </div>
);

export default SetsTab;
