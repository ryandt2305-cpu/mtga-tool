// EdhrecStrip — collapsible section docked inside DeckList (below the header).
// Renders the top 12 EDHREC cards for the current commander (by inclusion),
// greyed if unowned, clickable to pin as must-include. Also exposes the
// "Use EDHREC as seed" toggle that flips the request's seed to {kind:"edhrec"}.

import { type Component, For, Show, createMemo, createSignal } from "solid-js";
import type { EdhrecCard } from "../../lib/tauri";
import {
  anchors,
  community,
  seed,
  setSeed,
  togglePin,
} from "../../stores/deckStore";
import { allCards, cardsByName } from "../../stores/collectionStore";
import { getCardImageUrl } from "../../stores/priceStore";
import Tooltip from "../../components/Tooltip";

const MAX_ROWS = 12;

const EdhrecStrip: Component = () => {
  const [open, setOpen] = createSignal(true);

  const rows = createMemo<EdhrecCard[]>(() => {
    const c = community();
    if (!c) return [];
    return [...c.edhrec]
      .sort((a, b) => b.inclusion - a.inclusion)
      .slice(0, MAX_ROWS);
  });

  const seedIsEdhrec = () => seed()?.kind === "edhrec";

  function resolveGrp(name: string): number | null {
    const m = cardsByName().get(name);
    return m?.[0]?.grp_id ?? null;
  }

  function onCardClick(name: string): void {
    const grp = resolveGrp(name);
    if (grp === null) return;
    togglePin(grp);
  }

  function toggleEdhrecSeed(e: MouseEvent): void {
    e.stopPropagation();
    setSeed(seedIsEdhrec() ? null : { kind: "edhrec" });
  }

  return (
    <Show when={community() !== null}>
      <div class="decks-edhrec">
        <button
          class="decks-edhrec-header decks-edhrec-header--toggle"
          onClick={() => setOpen(!open())}
        >
          <span class="decks-edhrec-title">
            <span class="decks-disclosure-caret">{open() ? "▾" : "▸"}</span>
            Popular with this commander
            <span class="decks-edhrec-source"> · EDHREC ({rows().length})</span>
            <Show when={community() && !community()!.available}>
              <span class="decks-edhrec-offline"> · offline</span>
            </Show>
          </span>
          <label class="decks-edhrec-seed-toggle" onClick={(e) => e.stopPropagation()}>
            <input
              type="checkbox"
              checked={seedIsEdhrec()}
              onChange={toggleEdhrecSeed as unknown as (e: Event) => void}
            />
            Use EDHREC as seed
          </label>
        </button>
        <Show when={open()}>
          <Show
            when={rows().length > 0}
            fallback={
              <div class="decks-edhrec-empty">
                No EDHREC data for this commander yet.
              </div>
            }
          >
            <div class="decks-edhrec-strip">
              <For each={rows()}>
                {(row) => {
                  const grp = createMemo(() => resolveGrp(row.name));
                  const card = () => (grp() !== null ? allCards()[grp()!] : undefined);
                  const pinned = () => grp() !== null && anchors().must.has(grp()!);
                  const owned = () => grp() !== null;
                  return (
                    <Tooltip
                      text={
                        owned()
                          ? `${row.name} — ${Math.round(row.inclusion * 100)}% of decks`
                          : `${row.name} — not on Arena`
                      }
                    >
                    <button
                      class={`decks-edhrec-card ${owned() ? "" : "decks-edhrec-card--unowned"} ${pinned() ? "decks-edhrec-card--pinned" : ""}`}
                      onClick={() => onCardClick(row.name)}
                      disabled={!owned()}
                    >
                      <Show
                        when={card() !== undefined}
                        fallback={<div class="decks-edhrec-card-fallback">{row.name}</div>}
                      >
                        <img
                          class="decks-edhrec-card-img"
                          src={getCardImageUrl(card()!, "small")}
                          alt={row.name}
                          onError={(e) => {
                            const img = e.currentTarget;
                            const fallback = document.createElement("div");
                            fallback.className = "decks-edhrec-card-fallback";
                            fallback.textContent = row.name;
                            img.replaceWith(fallback);
                          }}
                        />
                      </Show>
                      <div class="decks-edhrec-card-name">{row.name}</div>
                      <div class="decks-edhrec-card-inc">
                        {Math.round(row.inclusion * 100)}%
                      </div>
                      <Show when={pinned()}>
                        <div class="decks-edhrec-card-pin">📌</div>
                      </Show>
                    </button>
                    </Tooltip>
                  );
                }}
              </For>
            </div>
          </Show>
        </Show>
      </div>
    </Show>
  );
};

export default EdhrecStrip;
