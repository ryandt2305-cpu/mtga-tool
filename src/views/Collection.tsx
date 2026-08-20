// Collection view — card grid with virtual scrolling and filters.

import { Show, createMemo, type Component } from "solid-js";
import Header from "../components/Header";
import FilterBar from "../components/FilterBar";
import VirtualGrid from "../components/VirtualGrid";
import CardCell from "../components/CardCell";
import ExportPopup from "../components/ExportPopup";
import {
  hasCollection,
  isScanning,
  scanProgress,
  scanStats,
  filteredCards,
  sortField,
  sortDirection,
  doScan,
} from "../stores/collectionStore";
import {
  totalCollectionValue,
  pricesLoaded,
  currencySymbol,
  getCardPrice,
} from "../stores/priceStore";
import { connectionLevel } from "../stores/connectionStore";

const RARITY_ORDER: Record<string, number> = {
  common: 0, uncommon: 1, rare: 2, mythic: 3,
  basic_land: -1, special: -2,
};

function nullsLast(a: number | null, b: number | null): number {
  if (a === null && b === null) return 0;
  if (a === null) return 1;
  if (b === null) return -1;
  return a - b;
}

const CARD_WIDTH = 180;
const CARD_HEIGHT = 251;
const CARD_GAP = 12;

const Collection: Component = () => {
  const sortedCards = createMemo(() => {
    const items = [...filteredCards()];
    const field = sortField();
    const dir = sortDirection() === "asc" ? 1 : -1;

    items.sort((a, b) => {
      let cmp = 0;
      switch (field) {
        case "set":
          cmp = a.card.set_code.localeCompare(b.card.set_code)
            || (parseInt(a.card.collector_number, 10) || 0) - (parseInt(b.card.collector_number, 10) || 0);
          break;
        case "name":
          cmp = a.card.name.localeCompare(b.card.name);
          break;
        case "price": {
          const ap = getCardPrice(a.card.grp_id, a.card.rarity);
          const bp = getCardPrice(b.card.grp_id, b.card.rarity);
          cmp = nullsLast(ap, bp);
          break;
        }
        case "cmc":
          cmp = a.card.cmc - b.card.cmc;
          break;
        case "rarity":
          cmp = (RARITY_ORDER[a.card.rarity] ?? -99) - (RARITY_ORDER[b.card.rarity] ?? -99);
          break;
        case "power":
          cmp = nullsLast(a.card.power, b.card.power);
          break;
        case "toughness":
          cmp = nullsLast(a.card.toughness, b.card.toughness);
          break;
        case "count":
          cmp = a.count - b.count;
          break;
      }
      if (cmp === 0 && field !== "name") cmp = a.card.name.localeCompare(b.card.name);
      return cmp * dir;
    });

    return items;
  });

  const isOffline = () => connectionLevel() === "offline";

  return (
    <div class="view">
      <Header title="Collection" staleBadge={isOffline()}>
        <Show when={hasCollection()}>
          <span class="collection-stats">
            {scanStats().uniqueCards.toLocaleString()} unique
            {" \u00B7 "}
            {scanStats().totalCopies.toLocaleString()} total
            <Show when={pricesLoaded()}>
              {" \u00B7 "}
              {currencySymbol()}{totalCollectionValue().toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 })} value
            </Show>
          </span>
          <ExportPopup cards={filteredCards} />
        </Show>
      </Header>

      <Show
        when={hasCollection()}
        fallback={
          <div class="view-content">
            <div class="scan-prompt">
              <p class="scan-prompt-text">
                {isOffline()
                  ? "No collection data yet — open MTGA to sync."
                  : "Scan MTGA's process memory to load your card collection. Make sure the game is running and your collection is loaded."}
              </p>
              <button
                class="scan-btn"
                disabled={isScanning() || isOffline()}
                title={isOffline() ? "MTGA is not running" : undefined}
                onClick={doScan}
              >
                {isScanning() ? "Scanning..." : "Scan Collection"}
              </button>
              <Show when={isScanning() && scanProgress().total > 0}>
                <span class="scan-progress">
                  Region {scanProgress().current} / {scanProgress().total}
                </span>
              </Show>
            </div>
          </div>
        }
      >
        <FilterBar />
        <div class="collection-grid-container">
          <VirtualGrid
            items={sortedCards()}
            itemWidth={CARD_WIDTH}
            itemHeight={CARD_HEIGHT}
            gap={CARD_GAP}
            renderItem={(entry) => (
              <CardCell card={entry.card} count={entry.count} />
            )}
          />
        </div>
      </Show>
    </div>
  );
};

export default Collection;
