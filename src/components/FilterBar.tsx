// Filter bar for collection view — search, set, rarity, ownership filters.

import { type Component } from "solid-js";
import {
  searchQuery,
  setSearchQuery,
  selectedSet,
  setSelectedSet,
  selectedRarities,
  setSelectedRarities,
  ownershipFilter,
  setOwnershipFilter,
  hideLands,
  setHideLands,
  sortField,
  setSortField,
  sortDirection,
  setSortDirection,
  availableSets,
  filteredCards,
  totalInCollection,
  type OwnershipFilter,
  type SortField,
} from "../stores/collectionStore";
import {
  selectedCurrency,
  setSelectedCurrency,
  type Currency,
} from "../stores/priceStore";

let debounceTimer: ReturnType<typeof setTimeout> | undefined;

function onSearchInput(value: string) {
  clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => setSearchQuery(value), 200);
}

function toggleRarity(rarity: string) {
  setSelectedRarities((prev) => {
    const next = new Set(prev);
    if (next.has(rarity)) {
      next.delete(rarity);
    } else {
      next.add(rarity);
    }
    return next;
  });
}

const RARITIES = [
  { key: "common", icon: "/icons/Nav_WildCard_Common.png", activeClass: "active-common" },
  { key: "uncommon", icon: "/icons/Nav_WildCard_Uncommon.png", activeClass: "active-uncommon" },
  { key: "rare", icon: "/icons/Nav_WildCard_Rare.png", activeClass: "active-rare" },
  { key: "mythic", icon: "/icons/Nav_WildCard_MythicRare.png", activeClass: "active-mythic" },
] as const;

const FilterBar: Component = () => {
  return (
    <div class="filter-bar">
      <input
        class="filter-input"
        type="text"
        placeholder="Search cards..."
        value={searchQuery()}
        onInput={(e) => onSearchInput(e.currentTarget.value)}
      />

      <select
        class="filter-select"
        value={selectedSet()}
        onChange={(e) => setSelectedSet(e.currentTarget.value)}
      >
        <option value="">All sets</option>
        {availableSets().map((set) => (
          <option value={set}>{set}</option>
        ))}
      </select>

      <div class="rarity-pills">
        {RARITIES.map((r) => (
          <button
            class={`rarity-pill ${selectedRarities().has(r.key) ? r.activeClass : ""}`}
            onClick={() => toggleRarity(r.key)}
          >
            <img src={r.icon} alt={r.key} class="rarity-pill-icon" />
          </button>
        ))}
      </div>

      <select
        class="filter-select"
        value={ownershipFilter()}
        onChange={(e) =>
          setOwnershipFilter(e.currentTarget.value as OwnershipFilter)
        }
      >
        <option value="all">All</option>
        <option value="owned">Owned 1+</option>
        <option value="playset">Playset 4</option>
        <option value="incomplete">Incomplete &lt;4</option>
      </select>

      <select
        class="filter-select"
        value={selectedCurrency()}
        onChange={(e) =>
          setSelectedCurrency(e.currentTarget.value as Currency)
        }
      >
        <option value="usd">USD</option>
        <option value="eur">EUR</option>
        <option value="aud">AUD</option>
      </select>

      <select
        class="filter-select"
        value={sortField()}
        onChange={(e) => setSortField(e.currentTarget.value as SortField)}
      >
        <option value="set">Set</option>
        <option value="name">Name</option>
        <option value="price">Price</option>
        <option value="cmc">Mana Value</option>
        <option value="rarity">Rarity</option>
        <option value="power">Power</option>
        <option value="toughness">Toughness</option>
        <option value="count">Count</option>
      </select>

      <button
        class="sort-dir-btn"
        title={sortDirection() === "asc" ? "Ascending" : "Descending"}
        onClick={() => setSortDirection((d) => d === "asc" ? "desc" : "asc")}
      >
        <svg width="14" height="14" viewBox="0 0 14 14" fill="currentColor"
          style={{ transform: sortDirection() === "desc" ? "scaleY(-1)" : "none" }}>
          <path d="M7 2 L12 9 H2 Z" />
        </svg>
      </button>

      <label class="filter-toggle">
        <input
          type="checkbox"
          checked={hideLands()}
          onChange={(e) => setHideLands(e.currentTarget.checked)}
        />
        Hide Lands
      </label>

      <span class="filter-count">
        Showing {filteredCards().length} of {totalInCollection()}
      </span>
    </div>
  );
};

export default FilterBar;
