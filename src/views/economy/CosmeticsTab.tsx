// Cosmetics tab — summary grid + art style detail with set grouping.
// Extracted from Economy.tsx for file size management.

import { type Component, Show, For, createSignal } from "solid-js";
import {
  cosmetics,
  artStyleCount,
  avatarCount,
  sleeveCount,
  petCount,
  emoteCount,
  titleCount,
  artStylesBySet,
} from "../../stores/cosmeticsStore";

// --- Icons + labels ---

const COSMETIC_ICONS: Record<string, string> = {
  art_styles: "/icons/ObjectiveIcon_OrbAndCardback.png",
  avatars: "/icons/ObjectiveIcon_AvatarMKM.png",
  sleeves: "/icons/ObjectiveIcon_Cardback_Basic_B_MKM.png",
  pets: "/icons/ObjectiveIcon_OrbAndCardback.png",
  emotes: "/icons/ObjectiveIcon_OrbAndCardback.png",
  titles: "/icons/ObjectiveIcon_OrbAndCardback.png",
};

const COSMETIC_LABELS: Record<string, string> = {
  art_styles: "Card Styles",
  avatars: "Avatars",
  sleeves: "Card Sleeves",
  pets: "Pets",
  emotes: "Emotes",
  titles: "Titles",
};

function scryfallSetSvg(setCode: string): string {
  return `https://svgs.scryfall.io/sets/${setCode.toLowerCase()}.svg`;
}

// --- Component ---

const CosmeticsTab: Component = () => {
  const c = () => cosmetics()!;

  const knownCategories = () => [
    { key: "art_styles", label: COSMETIC_LABELS.art_styles, count: artStyleCount(), icon: COSMETIC_ICONS.art_styles },
    { key: "avatars", label: COSMETIC_LABELS.avatars, count: avatarCount(), icon: COSMETIC_ICONS.avatars },
    { key: "sleeves", label: COSMETIC_LABELS.sleeves, count: sleeveCount(), icon: COSMETIC_ICONS.sleeves },
    { key: "pets", label: COSMETIC_LABELS.pets, count: petCount(), icon: COSMETIC_ICONS.pets },
    { key: "emotes", label: COSMETIC_LABELS.emotes, count: emoteCount(), icon: COSMETIC_ICONS.emotes },
    { key: "titles", label: COSMETIC_LABELS.titles, count: titleCount(), icon: COSMETIC_ICONS.titles },
  ];

  const otherCategories = () => {
    const other = c().other ?? {};
    return Object.entries(other).map(([key, values]) => ({
      key,
      label: key.replace(/([a-z])([A-Z])/g, "$1 $2"),
      count: Array.isArray(values) ? values.length : 0,
      icon: "/icons/ObjectiveIcon_OrbAndCardback.png",
    }));
  };

  return (
    <>
      {/* Summary grid */}
      <section class="economy-section">
        <h2 class="economy-section-title">
          <img class="economy-section-icon" src="/icons/ObjectiveIcon_AvatarMKM.png" alt="" />
          Cosmetics
        </h2>
        <div class="cosmetics-grid">
          <For each={knownCategories()}>
            {(cat) => (
              <div class="cosmetics-category">
                <img class="cosmetics-category-icon" src={cat.icon} alt="" />
                <span class="cosmetics-category-count">{cat.count}</span>
                <span class="cosmetics-category-label">{cat.label}</span>
              </div>
            )}
          </For>
          <For each={otherCategories()}>
            {(cat) => (
              <div class="cosmetics-category">
                <img class="cosmetics-category-icon" src={cat.icon} alt="" />
                <span class="cosmetics-category-count">{cat.count}</span>
                <span class="cosmetics-category-label">{cat.label}</span>
              </div>
            )}
          </For>
        </div>
      </section>

      {/* Art style detail */}
      <Show when={artStylesBySet().length > 0}>
        <ArtStyleDetail />
      </Show>
    </>
  );
};

// --- Art Style Detail ---

const ArtStyleDetail: Component = () => {
  const groups = () => artStylesBySet();
  const total = () => groups().reduce((sum, g) => sum + g.items.length, 0);

  return (
    <section class="art-style-section">
      <h2 class="art-style-section-header">
        <img class="economy-section-icon" src="/icons/ObjectiveIcon_OrbAndCardback.png" alt="" />
        Card Styles &middot; {total()}
      </h2>
      <For each={groups()}>
        {(group) => <SetGroup set={group.set} count={group.items.length} items={group.items} />}
      </For>
    </section>
  );
};

// --- Set Group (collapsible) ---

interface SetGroupProps {
  set: string;
  count: number;
  items: { grp_id: number; name: string; rarity: string }[];
}

const SetGroup: Component<SetGroupProps> = (props) => {
  const [expanded, setExpanded] = createSignal(false);

  return (
    <div class="art-style-set-group">
      <div class="art-style-set-header" onClick={() => setExpanded(!expanded())}>
        <img class="art-style-set-icon" src={scryfallSetSvg(props.set)} alt="" />
        <span class="art-style-set-name">{props.set}</span>
        <span class="art-style-set-count">&times;{props.count}</span>
        <span class={`art-style-set-chevron ${expanded() ? "expanded" : ""}`}>&rsaquo;</span>
      </div>
      <Show when={expanded()}>
        <div class="art-style-list">
          <For each={props.items}>
            {(style) => (
              <div class="art-style-row">
                <span class={`rarity-dot ${style.rarity}`} />
                <span>{style.name}</span>
              </div>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
};

export default CosmeticsTab;
