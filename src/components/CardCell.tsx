// Single card cell — Scryfall image with loading states, price badge, and count badge.
// Retries on error (handles Scryfall 429 rate limits) with exponential backoff.

import { createSignal, createMemo, onCleanup, Show, type Component } from "solid-js";
import type { CardInfo } from "../lib/tauri";
import { getCardPrice, getCardPriceSource, getCardImageUrl, currencySymbol, pricesLoaded } from "../stores/priceStore";
import { setOverlayCards } from "../stores/collectionStore";

interface CardCellProps {
  card: CardInfo;
  count: number;
}

const MAX_RETRIES = 3;
const LOAD_TIMEOUT_MS = 15_000;

function rarityClass(rarity: string): string {
  switch (rarity) {
    case "mythic":
      return "rarity-mythic";
    case "rare":
      return "rarity-rare";
    case "uncommon":
      return "rarity-uncommon";
    default:
      return "";
  }
}

const WC_NAV_ICONS: Record<string, string> = {
  common: "/icons/Nav_WildCard_Common.png",
  uncommon: "/icons/Nav_WildCard_Uncommon.png",
  rare: "/icons/Nav_WildCard_Rare.png",
  mythic: "/icons/Nav_WildCard_MythicRare.png",
};

const CardCell: Component<CardCellProps> = (props) => {
  const price = createMemo(() => getCardPrice(props.card.grp_id, props.card.rarity));
  const priceSource = createMemo(() => getCardPriceSource(props.card.grp_id, props.card.rarity));

  const [loaded, setLoaded] = createSignal(false);
  const [failed, setFailed] = createSignal(false);
  const [imgSrc, setImgSrc] = createSignal(getCardImageUrl(props.card, "normal"));
  let retries = 0;
  let loadTimer: ReturnType<typeof setTimeout> | null = null;
  let retryTimer: ReturnType<typeof setTimeout> | null = null;

  function clearLoadTimer() {
    if (loadTimer !== null) {
      clearTimeout(loadTimer);
      loadTimer = null;
    }
  }

  function clearRetryTimer() {
    if (retryTimer !== null) {
      clearTimeout(retryTimer);
      retryTimer = null;
    }
  }

  function startLoadTimer() {
    clearLoadTimer();
    loadTimer = setTimeout(() => {
      // Image never resolved (stalled connection) — treat as error
      if (!loaded() && !failed()) handleError();
    }, LOAD_TIMEOUT_MS);
  }

  // Start timer for the initial load
  startLoadTimer();
  onCleanup(() => { clearLoadTimer(); clearRetryTimer(); });

  function handleError() {
    clearLoadTimer();
    if (retries < MAX_RETRIES) {
      retries++;
      const delay = 1000 * Math.pow(2, retries - 1); // 1s, 2s, 4s
      retryTimer = setTimeout(() => {
        retryTimer = null;
        // Append cache-buster to force retry
        const base = getCardImageUrl(props.card, "normal");
        const sep = base.includes("?") ? "&" : "?";
        setImgSrc(base + sep + `_r=${retries}`);
        startLoadTimer();
      }, delay);
    } else {
      setFailed(true);
    }
  }

  return (
    <div class={`card-cell ${rarityClass(props.card.rarity)}`} onClick={() => setOverlayCards([props.card])}>
      <Show when={!failed()}>
        <Show when={!loaded()}>
          <div class="card-placeholder" />
        </Show>
        <img
          src={imgSrc()}
          alt={props.card.name}
          width="100%"
          height="100%"
          style={{
            opacity: loaded() ? "1" : "0",
            position: loaded() ? "static" : "absolute",
          }}
          onLoad={() => { clearLoadTimer(); setLoaded(true); }}
          onError={handleError}
        />
      </Show>
      <Show when={failed()}>
        <div class="card-fallback">{props.card.name}</div>
      </Show>

      {/* Paper price */}
      <Show when={price() !== null && priceSource() === "paper"}>
        <div class="card-price">{currencySymbol()}{price()!.toFixed(2)}</div>
      </Show>

      {/* Wildcard price */}
      <Show when={price() !== null && priceSource() === "wildcard"}>
        <div class="card-price card-price-wildcard">
          <img class="wc-icon" src={WC_NAV_ICONS[props.card.rarity]} alt="" />
          {currencySymbol()}{price()!.toFixed(2)}
        </div>
        <div class="card-arena-label">(Arena Only)</div>
      </Show>

      {/* No price (shouldn't happen after wildcard fallback, but safety net) */}
      <Show when={price() === null && pricesLoaded()}>
        <div class="card-price card-price-na">N/A</div>
      </Show>

      <div class="card-count">&times;{props.count}</div>
    </div>
  );
};

export default CardCell;
