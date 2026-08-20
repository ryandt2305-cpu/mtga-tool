// AlternativesPopover — anchored popover listing swap candidates for a slot.
// Row click issues rescoreKeeping(current slots minus swapped-out grp plus
// chosen grp) via deckStoreActions.rescoreKeeping().

import { type Component, For, Show, createMemo } from "solid-js";
import type { Alternative, Rarity, Slot } from "../../lib/tauri";
import { result } from "../../stores/deckStore";
import { rescoreKeeping } from "../../stores/deckStoreActions";
import { allCards } from "../../stores/collectionStore";
import { setOverlayCards } from "../../stores/collectionStore";

const WC_ICON: Record<string, string> = {
  common: "/icons/ObjectiveIcon_Wildcard_Common.png",
  uncommon: "/icons/ObjectiveIcon_Wildcard_Uncommon.png",
  rare: "/icons/ObjectiveIcon_Wildcard_Rare.png",
  mythic: "/icons/ObjectiveIcon_Wildcard_MythicRare.png",
};

export function wildcardIconPath(r: Rarity | null): string | null {
  if (r === null) return null;
  if (typeof r === "string") return WC_ICON[r] ?? null;
  return null;
}

interface Props {
  slot: Slot;
  onClose: () => void;
}

const AlternativesPopover: Component<Props> = (props) => {
  const list = createMemo(() => props.slot.alternatives.slice(0, 12));
  const maxScore = createMemo(() =>
    list().reduce((m, a) => (a.score > m ? a.score : m), 0.0001),
  );

  function choose(alt: Alternative): void {
    const cur = result();
    if (cur === null) return;
    // Keep everything except the swapped-out slot; add the alternative.
    const keep = cur.slots
      .filter((s) => s.grp_id !== props.slot.grp_id)
      .map((s) => s.grp_id);
    keep.push(alt.grp_id);
    void rescoreKeeping(keep);
    props.onClose();
  }

  function openOverlay(grp: number, e: MouseEvent): void {
    e.stopPropagation();
    const card = allCards()[grp];
    if (card !== undefined) setOverlayCards([card]);
  }

  return (
    <div class="decks-alts" onClick={(e) => e.stopPropagation()}>
      <div class="decks-alts-head">
        <span>Swap {props.slot.name}</span>
        <button class="decks-alts-close" onClick={props.onClose} aria-label="Close">×</button>
      </div>
      <Show
        when={list().length > 0}
        fallback={<div class="decks-alts-empty">No alternatives suggested.</div>}
      >
        <ul class="decks-alts-list">
          <For each={list()}>
            {(alt) => {
              const wc = wildcardIconPath(alt.wildcard_rarity);
              const bar = () => Math.round((alt.score / maxScore()) * 100);
              return (
                <li class={`decks-alts-row ${alt.owned ? "" : "decks-alts-row--unowned"}`} onClick={() => choose(alt)}>
                  <span
                    class="decks-alts-name"
                    onClick={(e) => openOverlay(alt.grp_id, e as unknown as MouseEvent)}
                    title="Open card"
                  >
                    {alt.name}
                  </span>
                  <span class="decks-alts-own">
                    <Show when={!alt.owned && wc !== null}>
                      <img class="decks-wc-icon" src={wc!} alt="wildcard" />
                    </Show>
                  </span>
                  <span class="decks-alts-bar"><span style={{ width: `${bar()}%` }} /></span>
                  <span class="decks-alts-reasons">{alt.reasons.slice(0, 2).join(" · ")}</span>
                </li>
              );
            }}
          </For>
        </ul>
      </Show>
    </div>
  );
};

export default AlternativesPopover;
