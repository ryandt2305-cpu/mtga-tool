// CommanderPicker — centre pane when step==="pick".
// Tabs: Commanders / Seed decks / Saved. Commanders grid reuses CardCell.
// Seed decks content is implemented in T18. Saved list is stubbed here and
// fleshed out in T17.

import { type Component, For, Show, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import CardCell from "../../components/CardCell";
import SavedDecks from "./SavedDecks";
import SeedList from "./SeedList";
import {
  candidates,
  candidatesLoading,
  commanderGrp,
  setCommanderGrp,
  setStep,
} from "../../stores/deckStore";
import { allCards } from "../../stores/collectionStore";
import {
  build,
  clearResult,
  loadCommunity,
  refreshCandidates,
} from "../../stores/deckStoreActions";
import type { CardInfo, CommanderCandidate } from "../../lib/tauri";

type PickerTab = "commanders" | "seeds" | "saved";

const CommanderPicker: Component = () => {
  const [tab, setTab] = createSignal<PickerTab>("commanders");
  const [search, setSearch] = createSignal("");

  function onSearch(v: string): void {
    setSearch(v);
    refreshCandidates(v.trim() === "" ? undefined : v.trim());
  }

  return (
    <div class="decks-picker">
      <div class="decks-picker-topbar">
        <input
          class="decks-picker-search"
          placeholder="Search commanders…"
          value={search()}
          onInput={(e) => onSearch((e.currentTarget as HTMLInputElement).value)}
        />
        <div class="decks-picker-tabs">
          <For each={[
            { id: "commanders" as PickerTab, label: "Commanders" },
            { id: "seeds" as PickerTab, label: "Seed decks" },
            { id: "saved" as PickerTab, label: "Saved" },
          ]}>
            {(t) => (
              <button
                class={`decks-picker-tab ${tab() === t.id ? "decks-picker-tab--active" : ""}`}
                onClick={() => setTab(t.id)}
              >
                {t.label}
              </button>
            )}
          </For>
        </div>
      </div>

      <Show when={tab() === "commanders"}>
        <CommandersGrid />
      </Show>
      <Show when={tab() === "seeds"}>
        <SeedList />
      </Show>
      <Show when={tab() === "saved"}>
        <SavedDecks />
      </Show>
    </div>
  );
};

// --- Commanders grid ---

const CommandersGrid: Component = () => {
  const rows = createMemo(() => candidates());

  return (
    <Show
      when={rows().length > 0}
      fallback={
        <div class="decks-empty">
          {candidatesLoading() ? "Loading commanders…" : "No commanders match — start MTGA to load your collection, or clear filters."}
        </div>
      }
    >
      <div class="decks-commanders">
        <For each={rows()}>
          {(cand) => <CommanderTile cand={cand} />}
        </For>
      </div>
    </Show>
  );
};

const CommanderTile: Component<{ cand: CommanderCandidate }> = (props) => {
  const card = createMemo<CardInfo | undefined>(() => allCards()[props.cand.grp_id]);
  const buildability = () => Math.round(props.cand.buildability * 100);
  const count = () => (props.cand.owned ? 1 : 0);

  function pick(e: Event): void {
    // Capture-phase click intercepts CardCell's own onClick (which would
    // otherwise open the card overlay).
    e.stopPropagation();
    e.preventDefault();
    if (commanderGrp() !== props.cand.grp_id) clearResult();
    setCommanderGrp(props.cand.grp_id);
    setStep("deck");
    void loadCommunity();
    void build();
  }

  let tile: HTMLDivElement | undefined;
  onMount(() => {
    tile?.addEventListener("click", pick, { capture: true });
  });
  onCleanup(() => {
    tile?.removeEventListener("click", pick, { capture: true });
  });

  return (
    <div
      ref={tile}
      class={`decks-cmd-tile ${props.cand.owned ? "" : "decks-cmd-tile--unowned"}`}
    >
      <Show
        when={card() !== undefined}
        fallback={<div class="decks-cmd-fallback">{props.cand.name}</div>}
      >
        <CardCell card={card()!} count={count()} />
      </Show>
      <div class="decks-cmd-badge">{buildability()}%</div>
      <div class="decks-cmd-name">{props.cand.name}</div>
    </div>
  );
};

export default CommanderPicker;
