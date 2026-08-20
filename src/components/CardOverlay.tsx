// Unified card overlay — two modes:
// 1. Single-card (from Collection): variants displayed horizontally (like the original overlay).
// 2. Multi-card (from Feed pack): cross layout — horizontal context cards, vertical variants.

import {
  createSignal,
  createMemo,
  createEffect,
  onCleanup,
  Show,
  For,
  type Component,
} from "solid-js";
import {
  overlayCards,
  setOverlayCards,
  overlayInitialIndex,
  cardsByName,
  collection,
} from "../stores/collectionStore";
import {
  getCardPrice,
  getCardPriceSource,
  getCardImageUrl,
  currencySymbol,
  pricesLoaded,
  fetchVariantPrices,
} from "../stores/priceStore";
import type { CardInfo } from "../lib/tauri";

const H_STRIDE = 282; // card width (250) + gap (32)
const FOCUS_TO_V = 311; // focused card center → first variant center (174 + 32 + 105)
const V_BETWEEN = 242; // between adjacent variant centers (105 + 32 + 105)

const WC_NAV_ICONS: Record<string, string> = {
  common: "/icons/Nav_WildCard_Common.png",
  uncommon: "/icons/Nav_WildCard_Uncommon.png",
  rare: "/icons/Nav_WildCard_Rare.png",
  mythic: "/icons/Nav_WildCard_MythicRare.png",
};

/** Sort printings chronologically by set_code then collector_number */
function sortPrintings(cards: CardInfo[]): CardInfo[] {
  return cards.slice().sort((a, b) =>
    a.set_code < b.set_code ? -1
      : a.set_code > b.set_code ? 1
      : Number(a.collector_number) - Number(b.collector_number)
  );
}

/** Vertical offset from cross center for variant at `idx` relative to focused variant at `focusIdx` */
function vTranslateY(idx: number, focusIdx: number): number {
  const rel = idx - focusIdx;
  if (rel === 0) return 0;
  const sign = rel > 0 ? 1 : -1;
  const abs = Math.abs(rel);
  return sign * (FOCUS_TO_V + (abs - 1) * V_BETWEEN);
}

const CardOverlay: Component = () => {
  const [focusedIndex, setFocusedIndex] = createSignal(0);
  const [slideDir, setSlideDir] = createSignal<"left" | "right" | null>(null);
  const [slideVDir, setSlideVDir] = createSignal<"up" | "down" | null>(null);

  /** Clear then re-apply direction so CSS animation restarts even for same-direction nav */
  function triggerSettle(dir: "left" | "right") {
    setSlideDir(null);
    requestAnimationFrame(() => setSlideDir(dir));
  }
  function triggerVSettle(dir: "up" | "down") {
    setSlideVDir(null);
    requestAnimationFrame(() => setSlideVDir(dir));
  }

  const cards = () => overlayCards();
  const isOpen = () => cards().length > 0;
  const focused = () => cards()[focusedIndex()];

  // Single-card mode: 1 card from Collection — show variants horizontally.
  // Multi-card mode: N cards from Feed — cross layout.
  const isSingleCard = () => cards().length === 1;

  // All printings of the focused card, sorted chronologically
  const variants = createMemo(() => {
    const f = focused();
    if (!f) return [];
    const index = cardsByName();
    return sortPrintings(index.get(f.name) ?? []);
  });

  // For single-card mode: the focused variant index within the horizontal strip
  const singleFocusedVariantIdx = createMemo(() => {
    const f = focused();
    if (!f) return 0;
    return Math.max(0, variants().findIndex((v) => v.grp_id === f.grp_id));
  });

  // For multi-card cross mode: index of current variant in variants array
  const focusedVariantIndex = createMemo(() => {
    const f = focused();
    if (!f) return -1;
    return variants().findIndex((v) => v.grp_id === f.grp_id);
  });

  // Reset focus only when transitioning from closed to open (not on every signal change)
  let prevLength = 0;
  createEffect(() => {
    const len = cards().length;
    if (len > 0 && prevLength === 0) setFocusedIndex(Math.min(overlayInitialIndex(), len - 1));
    prevLength = len;
  });

  // Fetch prices for all displayed + variant cards (deduplicated)
  createEffect(() => {
    const c = cards();
    const v = variants();
    if (c.length === 0) return;
    const seen = new Set<number>();
    const all: CardInfo[] = [];
    for (const card of [...c, ...v]) {
      if (!seen.has(card.grp_id)) {
        seen.add(card.grp_id);
        all.push(card);
      }
    }
    fetchVariantPrices(all);
  });

  /** Swap a vertical variant into the horizontal strip at the focused position */
  function swapVariant(variant: CardInfo) {
    const fi = focusedIndex();
    setOverlayCards((prev) => {
      const next = [...prev];
      next[fi] = variant;
      return next;
    });
  }

  // Keyboard navigation
  function handleKeyDown(e: KeyboardEvent) {
    if (!isOpen()) return;
    if (e.key === "Escape") {
      setOverlayCards([]);
    } else if (e.key === "ArrowLeft") {
      if (isSingleCard()) {
        const idx = singleFocusedVariantIdx();
        if (idx > 0) {
          triggerSettle("left");
          setOverlayCards([variants()[idx - 1]]);
        }
      } else {
        triggerSettle("left");
        setFocusedIndex((i) => Math.max(0, i - 1));
      }
    } else if (e.key === "ArrowRight") {
      if (isSingleCard()) {
        const idx = singleFocusedVariantIdx();
        if (idx < variants().length - 1) {
          triggerSettle("right");
          setOverlayCards([variants()[idx + 1]]);
        }
      } else {
        triggerSettle("right");
        setFocusedIndex((i) => Math.min(cards().length - 1, i + 1));
      }
    } else if (e.key === "ArrowUp" && !isSingleCard()) {
      const idx = focusedVariantIndex();
      if (idx > 0) { triggerVSettle("up"); swapVariant(variants()[idx - 1]); }
    } else if (e.key === "ArrowDown" && !isSingleCard()) {
      const idx = focusedVariantIndex();
      const v = variants();
      if (idx >= 0 && idx < v.length - 1) { triggerVSettle("down"); swapVariant(v[idx + 1]); }
    }
  }

  createEffect(() => {
    if (isOpen()) {
      window.addEventListener("keydown", handleKeyDown);
    } else {
      window.removeEventListener("keydown", handleKeyDown);
    }
  });
  onCleanup(() => window.removeEventListener("keydown", handleKeyDown));

  // Scroll navigation
  let wheelCooldown = false;

  function handleWheel(e: WheelEvent) {
    e.preventDefault();
    if (!isOpen() || wheelCooldown) return;

    const dy = e.deltaY;
    const dx = e.deltaX;
    const absDy = Math.abs(dy);
    const absDx = Math.abs(dx);

    if (isSingleCard()) {
      // Any scroll navigates variants
      const delta = absDy > absDx ? dy : dx;
      if (Math.abs(delta) < 5) return;
      const idx = singleFocusedVariantIdx();
      if (delta > 0 && idx < variants().length - 1) {
        triggerSettle("right");
        setOverlayCards([variants()[idx + 1]]);
      } else if (delta < 0 && idx > 0) {
        triggerSettle("left");
        setOverlayCards([variants()[idx - 1]]);
      }
    } else {
      const onFocused = !!(e.target as HTMLElement)?.closest?.(".card-overlay-item.focused");
      if (onFocused) {
        // Scroll on focused card → vertical variant navigation
        if (absDy < 5) return;
        const idx = focusedVariantIndex();
        const v = variants();
        if (dy > 0 && idx >= 0 && idx < v.length - 1) {
          triggerVSettle("down");
          swapVariant(v[idx + 1]);
        } else if (dy < 0 && idx > 0) {
          triggerVSettle("up");
          swapVariant(v[idx - 1]);
        }
      } else {
        // Scroll elsewhere → horizontal pack navigation
        const delta = absDy > absDx ? dy : dx;
        if (Math.abs(delta) < 5) return;
        if (delta > 0) {
          triggerSettle("right");
          setFocusedIndex((i) => Math.min(cards().length - 1, i + 1));
        } else {
          triggerSettle("left");
          setFocusedIndex((i) => Math.max(0, i - 1));
        }
      }
    }

    wheelCooldown = true;
    setTimeout(() => { wheelCooldown = false; }, 200);
  }

  function onBackdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) setOverlayCards([]);
  }

  return (
    <Show when={isOpen()}>
      <div
        class="card-overlay-backdrop"
        onClick={onBackdropClick}
        ref={(el) => el.addEventListener("wheel", handleWheel, { passive: false })}
      >
        <Show
          when={!isSingleCard()}
          fallback={
            /* --- Single-card mode: horizontal variant strip (Collection view) --- */
            <SingleCardLayout
              variants={variants()}
              focusedIdx={singleFocusedVariantIdx()}
              slideDir={slideDir()}
              onSelect={(card) => setOverlayCards([card])}
            />
          }
        >
          {/* --- Multi-card mode: cross layout (Feed view) --- */}
          <div class="card-overlay-cross">
            <div class="card-overlay-v-track">
              <For each={variants()}>
                {(variant, i) => (
                  <Show when={i() !== focusedVariantIndex()}>
                    <VCard
                      card={variant}
                      translateY={vTranslateY(i(), focusedVariantIndex())}
                      slideVDir={slideVDir()}
                      onSelect={() => swapVariant(variant)}
                    />
                  </Show>
                )}
              </For>
            </div>

            <div class="card-overlay-h-strip">
              <For each={cards()}>
                {(card, i) => (
                  <HCard
                    card={card}
                    index={i()}
                    focusedIndex={focusedIndex()}
                    slideDir={slideDir()}
                    onSelect={() => setFocusedIndex(i())}
                  />
                )}
              </For>
            </div>
          </div>
        </Show>

        {/* Info bar */}
        <Show when={focused()}>
          {(f) => {
            const col = collection();
            const price = () => getCardPrice(f().grp_id, f().rarity);
            const count = () => col[f().grp_id] ?? 0;

            return (
              <div class="card-overlay-info">
                <div class="card-overlay-name">{f().name}</div>
                <div>
                  {f().set_code} &middot; {f().rarity}
                  <Show when={price() !== null}>
                    {" "}&middot; {currencySymbol()}{price()!.toFixed(2)}
                  </Show>
                  <Show when={count() > 0}>
                    {" "}&middot; &times;{count()} owned
                  </Show>
                </div>
              </div>
            );
          }}
        </Show>
      </div>
    </Show>
  );
};

// --- Single-card layout: horizontal variant strip (matches old CardDetailOverlay) ---

const SingleCardLayout: Component<{
  variants: CardInfo[];
  focusedIdx: number;
  slideDir: "left" | "right" | null;
  onSelect: (card: CardInfo) => void;
}> = (props) => (
  <div class="card-overlay-h-strip">
    <For each={props.variants}>
      {(card, i) => (
        <HCard
          card={card}
          index={i()}
          focusedIndex={props.focusedIdx}
          slideDir={props.slideDir}
          onSelect={() => props.onSelect(card)}
        />
      )}
    </For>
  </div>
);

// --- Horizontal card item ---

const HCard: Component<{
  card: CardInfo;
  index: number;
  focusedIndex: number;
  slideDir: "left" | "right" | null;
  onSelect: () => void;
}> = (props) => {
  const isFocused = () => props.index === props.focusedIndex;
  const settleClass = () => isFocused() && props.slideDir ? ` settle-${props.slideDir}` : "";
  const count = () => collection()[props.card.grp_id] ?? 0;
  const isUnowned = () => count() === 0;
  const price = () => getCardPrice(props.card.grp_id, props.card.rarity);
  const source = () => getCardPriceSource(props.card.grp_id, props.card.rarity);
  const [imgLoaded, setImgLoaded] = createSignal(false);

  return (
    <div
      class={`card-overlay-item${isFocused() ? " focused" : ""}${settleClass()}${isUnowned() ? " unowned" : ""}`}
      style={{
        transform: (() => {
          const offset = props.index - props.focusedIndex;
          const tilt = Math.max(-3, Math.min(3, offset * -1.2));
          return `translateX(${offset * H_STRIDE}px) rotate(${tilt}deg)${isFocused() ? " scale(1.10)" : ""}`;
        })(),
      }}
      onClick={(e) => {
        e.stopPropagation();
        props.onSelect();
      }}
    >
      <Show when={!imgLoaded()}>
        <div class="card-overlay-shimmer" />
      </Show>
      <img
        src={getCardImageUrl(props.card, "normal")}
        alt={`${props.card.name} (${props.card.set_code})`}
        style={{ opacity: imgLoaded() ? "1" : "0", position: imgLoaded() ? "static" : "absolute" }}
        onLoad={() => setImgLoaded(true)}
      />

      {/* Price badge */}
      <Show when={price() !== null && source() === "paper"}>
        <div class="card-overlay-price">{currencySymbol()}{price()!.toFixed(2)}</div>
      </Show>
      <Show when={price() !== null && source() === "wildcard"}>
        <div class="card-overlay-price card-overlay-price-wildcard">
          <img class="wc-icon" src={WC_NAV_ICONS[props.card.rarity]} alt="" />
          {currencySymbol()}{price()!.toFixed(2)}
        </div>
      </Show>
      <Show when={price() === null && pricesLoaded()}>
        <div class="card-overlay-price card-overlay-price-na">N/A</div>
      </Show>

      <Show when={count() > 0}>
        <div class="card-overlay-count">&times;{count()}</div>
      </Show>
      <div class="card-overlay-set">{props.card.set_code}</div>
    </div>
  );
};

// --- Vertical variant card item ---

const VCard: Component<{
  card: CardInfo;
  translateY: number;
  slideVDir: "up" | "down" | null;
  onSelect: () => void;
}> = (props) => {
  const count = () => collection()[props.card.grp_id] ?? 0;
  const isUnowned = () => count() === 0;
  const price = () => getCardPrice(props.card.grp_id, props.card.rarity);
  const [imgLoaded, setImgLoaded] = createSignal(false);
  const tilt = () => Math.max(-2, Math.min(2, (props.translateY / FOCUS_TO_V) * -1));
  const settleClass = () => props.slideVDir ? ` settle-${props.slideVDir}` : "";

  return (
    <div
      class={`card-overlay-v-item${isUnowned() ? " unowned" : ""}${settleClass()}`}
      style={{ transform: `translate(-50%, -50%) translateY(${props.translateY}px) rotate(${tilt()}deg)` }}
      onClick={(e) => {
        e.stopPropagation();
        props.onSelect();
      }}
    >
      <Show when={!imgLoaded()}>
        <div class="card-overlay-v-shimmer" />
      </Show>
      <img
        src={getCardImageUrl(props.card, "normal")}
        alt={`${props.card.name} (${props.card.set_code})`}
        style={{ opacity: imgLoaded() ? "1" : "0", position: imgLoaded() ? "static" : "absolute" }}
        onLoad={() => setImgLoaded(true)}
      />
      <Show when={price() !== null}>
        <div class="card-overlay-v-price">{currencySymbol()}{price()!.toFixed(2)}</div>
      </Show>
      <div class="card-overlay-v-set">{props.card.set_code}</div>
    </div>
  );
};

export default CardOverlay;
