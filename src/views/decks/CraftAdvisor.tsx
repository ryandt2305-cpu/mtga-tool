// CraftAdvisor — "Craft next" disclosure: the engine's ranked wildcard
// upgrade suggestions (BuildResult.craft_suggestions). Rows the current
// budget can't afford render dimmed — the "craft when you earn wildcards"
// tier. Only OwnedOnly builds populate the list.

import { type Component, For, Show, createMemo } from "solid-js";
import Disclosure from "./Disclosure";
import { wcIconForRarity } from "./wildcardIcons";
import type { CraftSuggestion } from "../../lib/tauri";
import { result } from "../../stores/deckStore";

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

  return (
    <Show when={suggestions().length > 0}>
      <Disclosure label="Craft next" summary={summary}>
        <ul class="decks-craft-list">
          <For each={suggestions()}>
            {(s) => (
              <li
                class={`decks-craft-row ${s.affordable ? "" : "decks-craft-row--locked"}`}
                title={s.affordable ? undefined : "Not enough wildcards of this rarity yet"}
              >
                <Show when={wcIconForRarity(s.rarity)} keyed>
                  {(icon) => (
                    <img class="decks-wc-icon" src={icon} alt={String(s.rarity)} />
                  )}
                </Show>
                <div class="decks-craft-main">
                  <div class="decks-craft-name">{s.name}</div>
                  <div class="decks-stats-sub">
                    <Show when={s.replaces_name}>
                      <span>replaces {s.replaces_name} · </span>
                    </Show>
                    {s.reasons.slice(0, 3).join(" · ")}
                  </div>
                </div>
              </li>
            )}
          </For>
        </ul>
      </Disclosure>
    </Show>
  );
};

export default CraftAdvisor;
