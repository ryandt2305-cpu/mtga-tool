// Decks — top-level view. Header, contextual card-data strip, then a 3-column
// grid: SetupRail | centre (CommanderPicker or DeckList) | StatsRail.

import { type Component, Show } from "solid-js";
import Header from "../components/Header";
import SetupRail from "./decks/SetupRail";
import CommanderPicker from "./decks/CommanderPicker";
import DeckList from "./decks/DeckList";
import StatsRail from "./decks/StatsRail";
import DeckDataStrip from "./decks/DeckDataStrip";
import { step } from "../stores/deckStore";
import "../styles/decks.css";

const Decks: Component = () => (
  <div class="view">
    <Header title="Decks" />
    <div class="view-content decks-layout">
      <SetupRail />
      <section class="decks-center">
        <DeckDataStrip />
        <Show
          when={step() === "pick"}
          fallback={<DeckList />}
        >
          <CommanderPicker />
        </Show>
      </section>
      <StatsRail />
    </div>
  </div>
);

export default Decks;
