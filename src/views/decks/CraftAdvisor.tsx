// CraftAdvisor — "Craft next" disclosure: the engine's ranked wildcard
// upgrade suggestions (BuildResult.craft_suggestions). Rows the current
// budget can't afford render dimmed — the "craft when you earn wildcards"
// tier. Only OwnedOnly builds populate the list.

import { type Component, For, Show, createMemo } from "solid-js";
import Disclosure from "./Disclosure";
import { wcIconForRarity } from "./wildcardIcons";
import type { CraftSuggestion } from "../../lib/tauri";
import { result } from "../../stores/deckStore";
import { rescoreKeeping } from "../../stores/deckStoreActions";
import CardHoverPreview from "../../components/CardHoverPreview";
import Tooltip from "../../components/Tooltip";

const CraftAdvisor: Component = () => {
  const suggestions = createMemo<CraftSuggestion[]>(
    () => result()?.craft_suggestions ?? [],
  );

  const summary = () => {
    const s = suggestions();
    const affordable = s.filter((x) => x.affordable).length;
    return (
      <span class="decks-stats-sub">
        {affordable > 0 ? `${affordable} in budget` : "save wildcards"} · {s[0].name}
      </span>
    );
  };

  function swapIn(s: CraftSuggestion): void {
    const cur = result();
    if (cur === null || s.replaces_name === null) return;
    const target = s.replaces_name.toLowerCase();
    const keep = cur.slots
      .filter((slot) => slot.name.toLowerCase() !== target)
      .map((slot) => slot.grp_id);
    if (keep.length === cur.slots.length) return; // slot no longer present
    keep.push(s.grp_id);
    void rescoreKeeping(keep);
  }

  return (
    <Show when={suggestions().length > 0}>
      <Disclosure label="Craft next" summary={summary}>
        <ul class="decks-craft-list">
          <For each={suggestions()}>
            {(s) => (
              <li
                class={`decks-craft-row ${s.affordable ? "" : "decks-craft-row--locked"}`}
              >
                <Show when={wcIconForRarity(s.rarity)} keyed>
                  {(icon) => (
                    <img class="decks-wc-icon" src={icon} alt={String(s.rarity)} />
                  )}
                </Show>
                <div class="decks-craft-main">
                  <div class="decks-craft-name">
                    <CardHoverPreview name={s.name} class="decks-craft-name-link">
                      {s.name}
                    </CardHoverPreview>
                  </div>
                  <div class="decks-stats-sub">
                    <Show when={s.replaces_name}>
                      <span>
                        replaces{" "}
                        <CardHoverPreview
                          name={s.replaces_name!}
                          class="decks-craft-replaces-link"
                        >
                          {s.replaces_name}
                        </CardHoverPreview>
                        {" "}·{" "}
                      </span>
                    </Show>
                    {s.reasons.slice(0, 3).join(" · ")}
                  </div>
                </div>
                <Show when={s.replaces_name !== null}>
                  <Tooltip
                    text={
                      s.affordable
                        ? `Swap ${s.replaces_name} → ${s.name}`
                        : `Not enough ${s.rarity} wildcards yet — swap anyway`
                    }
                  >
                    <button
                      type="button"
                      class="decks-craft-swap"
                      onClick={() => swapIn(s)}
                      aria-label={`Swap ${s.replaces_name} for ${s.name}`}
                    >
                      ✓
                    </button>
                  </Tooltip>
                </Show>
              </li>
            )}
          </For>
        </ul>
      </Disclosure>
    </Show>
  );
};

export default CraftAdvisor;
