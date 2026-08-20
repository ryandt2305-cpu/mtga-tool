// Live Feed — real-time event stream from LogService and MemoryService.
// Shows inventory changes, card grants, collection diffs, and generic log events.
// Hero image layout: card art or MTGA icon fills the left column, text on the right.

import { type Component, Show, For, createSignal, onMount, onCleanup } from "solid-js";
import Header from "../components/Header";
import FeedHero from "../components/FeedHero";
import { connectionLevel, connectionLabel } from "../stores/connectionStore";
import { allCards, setOverlayDeck, type DeckOverlayData } from "../stores/collectionStore";
import type { CardInfo, DeckCard } from "../lib/tauri";
import {
  hasFeedEntries,
  filteredFeedEntries,
  activeFilters,
  toggleFilter,
  clearFilters,
  loadMoreEntries,
  isLoadingMore,
  hasMoreEntries,
  type FeedCategory,
  type FeedEntry,
  type DeltaItem,
} from "../stores/feedStore";

// --- Filter chip config ---

const CATEGORIES: { key: FeedCategory; label: string }[] = [
  { key: "economy", label: "Economy" },
  { key: "cards", label: "Cards" },
  { key: "event", label: "Events" },
  { key: "system", label: "System" },
];

// --- Relative time ---

function relativeTime(timestamp: number, now: number): string {
  const diff = Math.max(0, Math.floor((now - timestamp) / 1000));
  if (diff < 5) return "just now";
  if (diff < 60) return `${diff}s ago`;
  const minutes = Math.floor(diff / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

// --- Component ---

const Feed: Component = () => {
  const [now, setNow] = createSignal(Date.now());
  const timer = setInterval(() => setNow(Date.now()), 5000);
  onCleanup(() => clearInterval(timer));

  const isAllActive = () => activeFilters().size === 0;

  return (
    <div class="view">
      <Header title="Live Feed" />
      <div class="view-content">
        <div class="feed-filters">
          <button
            class={`feed-chip ${isAllActive() ? "active" : ""}`}
            onClick={clearFilters}
          >
            All
          </button>
          <For each={CATEGORIES}>
            {(cat) => (
              <button
                class={`feed-chip ${activeFilters().has(cat.key) ? "active" : ""}`}
                onClick={() => toggleFilter(cat.key)}
              >
                {cat.label}
              </button>
            )}
          </For>
        </div>

        <ConnectionStatus />

        <Show when={hasFeedEntries()} fallback={<EmptyState />}>
          <div class="feed-list">
            <For each={filteredFeedEntries()}>
              {(entry) =>
                entry.kind === "session_end"
                  ? <SessionEndDivider entry={entry} />
                  : <FeedRow entry={entry} now={now()} />
              }
            </For>
            <LoadSentinel />
          </div>
        </Show>
      </div>
    </div>
  );
};

// --- Empty state ---

const EmptyState: Component = () => {
  const isOffline = () => connectionLevel() === "offline";
  return (
    <div class="feed-empty">
      <span class="feed-empty-title">Waiting for events...</span>
      <span class="feed-empty-subtitle">
        {isOffline()
          ? "History from prior sessions is loaded. Open MTGA to record new events."
          : "Events from MTGA will appear here as they happen"}
      </span>
    </div>
  );
};

// --- Connection status ---

const ConnectionStatus: Component = () => {
  const dotColor = () => {
    const level = connectionLevel();
    if (level === "live") return "var(--color-status-ok)";
    if (level === "offline") return "var(--color-status-warn)";
    return "var(--color-status-error)";
  };

  return (
    <div class="feed-connection-status">
      <span class="feed-connection-dot" style={{ background: dotColor() }} />
      <span class="feed-connection-label">{connectionLabel()}</span>
    </div>
  );
};

// --- Session end divider ---

const SessionEndDivider: Component<{ entry: FeedEntry }> = (props) => (
  <div class="feed-session-divider">
    <div class="feed-session-divider-line" />
    <span class="feed-session-divider-text">{props.entry.title}</span>
    <div class="feed-session-divider-line" />
  </div>
);

// --- Load sentinel (infinite scroll) ---

const LoadSentinel: Component = () => {
  let sentinelRef: HTMLDivElement | undefined;

  onMount(() => {
    if (!sentinelRef) return;
    const scrollContainer = sentinelRef.closest(".view-content");
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting) {
          loadMoreEntries();
        }
      },
      { root: scrollContainer, rootMargin: "200px" },
    );
    observer.observe(sentinelRef);
    onCleanup(() => observer.disconnect());
  });

  return (
    <>
      <Show when={hasMoreEntries()}>
        <div ref={sentinelRef} class="feed-load-sentinel" />
      </Show>
      <Show when={isLoadingMore()}>
        <div class="feed-loading-more">Loading...</div>
      </Show>
    </>
  );
};

// --- Deck overlay helper ---

function resolveDeckCards(deckCards: DeckCard[]): CardInfo[] {
  const db = allCards();
  return deckCards
    .map((dc) => db[dc.grp_id])
    .filter((c): c is CardInfo => c != null);
}

function openDeckOverlay(entry: FeedEntry): void {
  const dd = entry.deckData;
  if (!dd) return;
  const data: DeckOverlayData = {
    name: dd.name,
    format: dd.format,
    mainboard: dd.mainboard ? resolveDeckCards(dd.mainboard) : [],
    sideboard: dd.sideboard ? resolveDeckCards(dd.sideboard) : [],
  };
  setOverlayDeck(data);
}

// --- Feed row ---

const FeedRow: Component<{ entry: FeedEntry; now: number }> = (props) => {
  const deckClick = () => props.entry.deckData ? () => openDeckOverlay(props.entry) : undefined;

  return (
    <div class="feed-row">
      <FeedHero thumbnails={props.entry.thumbnails} icon={props.entry.icon} onClick={deckClick()} />

      <div class="feed-row-body">
        <div class="feed-row-header">
          <span class="feed-row-title">{props.entry.title}</span>
          <span class="feed-row-time">
            {relativeTime(props.entry.timestamp, props.now)}
          </span>
        </div>

        <Show when={props.entry.details}>
          <span class="feed-row-details">{props.entry.details}</span>
        </Show>

        <Show when={props.entry.deltas && props.entry.deltas.length > 0}>
          <div class="feed-row-deltas">
            <For each={props.entry.deltas}>
              {(delta) => <DeltaPill delta={delta} />}
            </For>
          </div>
        </Show>
      </div>
    </div>
  );
};

// --- Delta pill ---

const DeltaPill: Component<{ delta: DeltaItem }> = (props) => {
  const isSnapshot = () => props.delta.mode === "snapshot";
  const sign = () => isSnapshot() ? "neutral" : (props.delta.value > 0 ? "positive" : "negative");
  const display = () => {
    if (isSnapshot()) return formatDelta(props.delta.value);
    return props.delta.value > 0 ? `+${formatDelta(props.delta.value)}` : formatDelta(props.delta.value);
  };

  return (
    <span class={`feed-delta ${sign()}`}>
      <Show when={props.delta.icon}>
        <img
          class="feed-delta-icon"
          src={`/icons/${props.delta.icon}`}
          alt=""
          onError={(e) => {
            (e.target as HTMLImageElement).style.display = "none";
          }}
        />
      </Show>
      {display()}
    </span>
  );
};

// --- Helpers ---

function formatDelta(n: number): string {
  return Math.abs(n) >= 1000 ? n.toLocaleString("en-US") : String(n);
}

export default Feed;
