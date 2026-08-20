// Deck overlay — full-screen deck card list viewer.
// Reads overlayDeck signal; when non-null, shows mainboard + sideboard grid.
// Individual cards are clickable to open the CardOverlay for variant browsing.

import {
  createSignal,
  createEffect,
  onCleanup,
  Show,
  For,
  type Component,
} from "solid-js";
import {
  overlayDeck,
  setOverlayDeck,
  setOverlayCards,
  collection,
} from "../stores/collectionStore";
import type { CardInfo } from "../lib/tauri";

function scryfallSmall(card: CardInfo): string {
  const set = card.set_code.toLowerCase();
  return `https://api.scryfall.com/cards/${set}/${card.collector_number}?format=image&version=small`;
}

/** Build display entries: deduplicate by grp_id and count occurrences. */
function buildDisplayCards(cards: CardInfo[]): { card: CardInfo; quantity: number }[] {
  const seen = new Map<number, { card: CardInfo; quantity: number }>();
  for (const card of cards) {
    const existing = seen.get(card.grp_id);
    if (existing) {
      existing.quantity++;
    } else {
      seen.set(card.grp_id, { card, quantity: 1 });
    }
  }
  return Array.from(seen.values());
}

const DeckOverlay: Component = () => {
  const deck = () => overlayDeck();
  const isOpen = () => deck() !== null;

  // Keyboard handler
  function handleKeyDown(e: KeyboardEvent) {
    if (!isOpen()) return;
    if (e.key === "Escape") {
      setOverlayDeck(null);
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

  function onBackdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) setOverlayDeck(null);
  }

  function handleCardClick(card: CardInfo) {
    setOverlayCards([card]);
  }

  return (
    <Show when={deck()}>
      {(d) => {
        const mainboard = () => buildDisplayCards(d().mainboard);
        const sideboard = () => buildDisplayCards(d().sideboard);
        const col = collection();

        return (
          <div class="deck-overlay-backdrop" onClick={onBackdropClick}>
            <div class="deck-overlay-container">
              <div class="deck-overlay-header">
                <span class="deck-overlay-name">{d().name}</span>
                <Show when={d().format}>
                  <span class="deck-overlay-format">{d().format}</span>
                </Show>
                <span class="deck-overlay-count">
                  {d().mainboard.length} cards
                  {d().sideboard.length > 0 ? ` + ${d().sideboard.length} sideboard` : ""}
                </span>
              </div>

              <div class="deck-overlay-section">
                <div class="deck-overlay-section-label">Mainboard</div>
                <div class="deck-overlay-grid">
                  <For each={mainboard()}>
                    {(entry) => (
                      <DeckCardItem
                        card={entry.card}
                        quantity={entry.quantity}
                        owned={col[entry.card.grp_id] ?? 0}
                        onClick={() => handleCardClick(entry.card)}
                      />
                    )}
                  </For>
                </div>
              </div>

              <Show when={sideboard().length > 0}>
                <div class="deck-overlay-section">
                  <div class="deck-overlay-section-label">Sideboard</div>
                  <div class="deck-overlay-grid">
                    <For each={sideboard()}>
                      {(entry) => (
                        <DeckCardItem
                          card={entry.card}
                          quantity={entry.quantity}
                          owned={col[entry.card.grp_id] ?? 0}
                          onClick={() => handleCardClick(entry.card)}
                        />
                      )}
                    </For>
                  </div>
                </div>
              </Show>
            </div>
          </div>
        );
      }}
    </Show>
  );
};

// --- Individual card in the grid ---

const DeckCardItem: Component<{
  card: CardInfo;
  quantity: number;
  owned: number;
  onClick: () => void;
}> = (props) => {
  const [imgLoaded, setImgLoaded] = createSignal(false);

  return (
    <div
      class={`deck-overlay-card${props.owned === 0 ? " unowned" : ""}`}
      onClick={(e) => {
        e.stopPropagation();
        props.onClick();
      }}
    >
      <Show when={!imgLoaded()}>
        <div class="deck-overlay-card-shimmer" />
      </Show>
      <img
        src={scryfallSmall(props.card)}
        alt={props.card.name}
        style={{ opacity: imgLoaded() ? "1" : "0", position: imgLoaded() ? "static" : "absolute" }}
        onLoad={() => setImgLoaded(true)}
      />
      <Show when={props.quantity > 1}>
        <span class="deck-overlay-qty">&times;{props.quantity}</span>
      </Show>
    </div>
  );
};

export default DeckOverlay;
