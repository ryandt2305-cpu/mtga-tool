// Fanned hero image for feed entries.
// 1 card: single art. 2 cards: stacked offset. 3: fan spread.
// 4+: 3 fanned cards + "+N" count badge.
// Sorted by rarity so the best pull is frontmost.

import { createMemo, Show, For, type Component } from "solid-js";
import { allCards, setOverlayCards } from "../stores/collectionStore";
import { getCardPrice, getCardImageUrl, currencySymbol, pricesLoaded } from "../stores/priceStore";
import type { CardInfo } from "../lib/tauri";
import type { CardThumbnail } from "../stores/feedStore";

interface FeedHeroProps {
  thumbnails?: CardThumbnail[];
  icon?: string;
  onClick?: () => void;
}

const RARITY_ORDER: Record<string, number> = {
  mythic: 0,
  rare: 1,
  uncommon: 2,
  common: 3,
};

function sortByRarity(thumbs: CardThumbnail[]): CardThumbnail[] {
  return [...thumbs].sort(
    (a, b) =>
      (RARITY_ORDER[a.rarity.toLowerCase()] ?? 4) -
      (RARITY_ORDER[b.rarity.toLowerCase()] ?? 4)
  );
}

const FeedHero: Component<FeedHeroProps> = (props) => {
  const hasCards = () => (props.thumbnails?.length ?? 0) > 0;

  // Sort by rarity for fanning (best pull frontmost)
  const sorted = createMemo(() => {
    const t = props.thumbnails;
    if (!t || t.length === 0) return [];
    return sortByRarity(t);
  });

  // Show at most 3 fanned cards
  const fanCards = createMemo(() => sorted().slice(0, 3));
  const extraCount = createMemo(() => {
    const total = props.thumbnails?.length ?? 0;
    return total > 3 ? total - 3 : 0;
  });

  // Highest rarity for border styling
  const heroRarity = createMemo((): string | null => {
    const s = sorted();
    return s.length > 0 ? s[0].rarity.toLowerCase() : null;
  });

  // Price badge on frontmost card
  const heroPrice = createMemo(() => {
    const s = sorted();
    if (s.length === 0) return null;
    return getCardPrice(s[0].grp_id, s[0].rarity);
  });

  const handleClick = () => {
    if (props.onClick) {
      props.onClick();
      return;
    }
    const thumbs = props.thumbnails;
    if (!thumbs || thumbs.length === 0) return;
    const cardDb = allCards();
    const resolved = sortByRarity(thumbs)
      .map((t) => cardDb[t.grp_id])
      .filter((c): c is CardInfo => c != null);
    if (resolved.length > 0) setOverlayCards(resolved);
  };

  return (
    <Show
      when={hasCards()}
      fallback={
        <Show when={props.icon}>
          <div class="feed-row-hero icon-art">
            <img
              src={`/icons/${props.icon}`}
              alt=""
              onError={(e) => {
                (e.target as HTMLImageElement).style.display = "none";
              }}
            />
          </div>
        </Show>
      }
    >
      <div
        class={`feed-row-hero card-art${heroRarity() ? ` rarity-${heroRarity()}` : ""}${fanCards().length > 1 ? " has-fan" : ""}`}
        onClick={handleClick}
      >
        <For each={fanCards()}>
          {(card, i) => (
            <img
              class={`feed-hero-fan-card fan-${i()} rarity-${card.rarity.toLowerCase()}`}
              src={getCardImageUrl(card, "small")}
              alt=""
              onError={(e) => {
                (e.target as HTMLImageElement).style.display = "none";
              }}
              onClick={(e) => {
                e.stopPropagation();
                const thumbs = props.thumbnails;
                if (!thumbs || thumbs.length === 0) return;
                const cardDb = allCards();
                // Resolve all cards, placing the clicked card first so it gets focus
                const clicked = cardDb[card.grp_id];
                if (!clicked) return;
                const rest = sortByRarity(thumbs)
                  .map((t) => cardDb[t.grp_id])
                  .filter((c): c is CardInfo => c != null && c.grp_id !== card.grp_id);
                setOverlayCards([clicked, ...rest]);
              }}
            />
          )}
        </For>
        <Show when={extraCount() > 0}>
          <span class="feed-row-hero-badge">+{extraCount()}</span>
        </Show>
        <Show when={heroPrice() !== null && pricesLoaded()}>
          <span class="feed-row-hero-price">
            {currencySymbol()}{heroPrice()!.toFixed(2)}
          </span>
        </Show>
      </div>
    </Show>
  );
};

export default FeedHero;
