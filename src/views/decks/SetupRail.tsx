// SetupRail — compact left column: summary of current build parameters,
// Build button, and 17Lands toggle. Full setup lives behind an Edit button
// that opens SetupModal. Keeps the primary action (Build) and the per-build
// modifier (17Lands) always one click away.

import { type Component, For, Show, createMemo, createSignal } from "solid-js";
import {
  format,
  ownership,
  playstyle,
  colorSubset,
  themeTags,
  templateOverrides,
  use17Lands,
  setUse17Lands,
  commanderGrp,
  result,
  building,
} from "../../stores/deckStore";
import { build } from "../../stores/deckStoreActions";
import type { Format } from "../../lib/tauri";
import Tooltip from "../../components/Tooltip";
import {
  activeTemplate,
  FormatControl,
  OwnershipControl,
  WildcardBudgetControl,
  PlaystyleControl,
  ColorPipsControl,
  ThemeTagsControl,
  AdvancedControl,
} from "./SetupControls";

const FORMAT_LABEL: Record<Format, string> = {
  brawl100: "Brawl 100",
  standardbrawl60: "Standard Brawl 60",
};

function ownershipLabel(): string {
  const o = ownership();
  if (o.mode === "best_deck") return "Best deck";
  const b = o.wc_budget;
  const total = b.common + b.uncommon + b.rare + b.mythic;
  return total > 0 ? `Owned + ${total} WC` : "Owned only";
}

function overrideCount(): number {
  const o = templateOverrides();
  if (o === null || o === undefined) return 0;
  let n = 0;
  if (o.lands !== undefined && o.lands !== null) n += 1;
  if (o.role_min) n += Object.keys(o.role_min).length;
  if (o.role_max) n += Object.keys(o.role_max).length;
  return n;
}

const SetupRail: Component = () => {
  const [modalOpen, setModalOpen] = createSignal(false);

  const colorText = createMemo(() => {
    const c = colorSubset();
    return c === null || c === "" ? "any colours" : c;
  });

  return (
    <>
      <aside class="decks-setup">
        <div class="decks-setup-summary">
          <div class="decks-setup-summary-row">
            <span class="decks-setup-summary-key">Format</span>
            <span class="decks-setup-summary-val">{FORMAT_LABEL[format()]}</span>
          </div>
          <div class="decks-setup-summary-row">
            <span class="decks-setup-summary-key">Cards</span>
            <span class="decks-setup-summary-val">{ownershipLabel()}</span>
          </div>
          <div class="decks-setup-summary-row">
            <span class="decks-setup-summary-key">Style</span>
            <span class="decks-setup-summary-val">
              <span class="decks-setup-summary-playstyle">{playstyle()}</span>
              <span class="decks-setup-summary-sep">·</span>
              <span class="decks-setup-summary-colors">{colorText()}</span>
            </span>
          </div>
          <Show when={themeTags().length > 0}>
            <div class="decks-setup-summary-row">
              <span class="decks-setup-summary-key">Themes</span>
              <span class="decks-setup-summary-val">
                {themeTags().length} tag{themeTags().length === 1 ? "" : "s"}
              </span>
            </div>
          </Show>
          <Show when={overrideCount() > 0}>
            <div class="decks-setup-summary-row">
              <span class="decks-setup-summary-key">Advanced</span>
              <span class="decks-setup-summary-val">
                {overrideCount()} override{overrideCount() === 1 ? "" : "s"}
              </span>
            </div>
          </Show>
        </div>

        <button class="decks-setup-edit" onClick={() => setModalOpen(true)}>
          Edit setup
        </button>

        <BuildButton />

        <label class="decks-checkbox decks-setup-17lands">
          <input
            type="checkbox"
            checked={use17Lands()}
            onChange={(e) => setUse17Lands((e.currentTarget as HTMLInputElement).checked)}
          />
          <span>17Lands ratings</span>
        </label>
      </aside>

      <Show when={modalOpen()}>
        <SetupModal onClose={() => setModalOpen(false)} />
      </Show>
    </>
  );
};

const BuildButton: Component = () => {
  const canBuild = () => commanderGrp() !== null && !building();
  const label = () => (result() === null ? "Build deck" : "Rebuild");
  return (
    <button
      class="decks-build"
      disabled={!canBuild()}
      onClick={() => void build()}
    >
      {building() ? "Building…" : label()}
    </button>
  );
};

const SetupModal: Component<{ onClose: () => void }> = (props) => {
  const tpl = createMemo(() => activeTemplate());

  function onBackdrop(e: MouseEvent): void {
    if (e.target === e.currentTarget) props.onClose();
  }

  return (
    <div class="decks-modal-backdrop" onClick={onBackdrop}>
      <div class="decks-modal decks-setup-modal">
        <div class="decks-modal-head">
          <span>Edit setup</span>
          <Tooltip text="Close"><button class="decks-alts-close" onClick={props.onClose}>×</button></Tooltip>
        </div>

        <div class="decks-setup-modal-body">
          <SetupSection title="Format & ownership">
            <FormatControl />
            <OwnershipControl />
            <Show when={ownership().mode === "owned_only"}>
              <WildcardBudgetControl />
            </Show>
          </SetupSection>

          <SetupSection title="Strategy">
            <PlaystyleControl />
            <ColorPipsControl />
            <ThemeTagsControl />
          </SetupSection>

          <SetupSection title="Advanced">
            <AdvancedControl template={tpl()} />
          </SetupSection>
        </div>

        <div class="decks-setup-modal-foot">
          <button class="decks-btn decks-btn--primary" onClick={props.onClose}>
            Done
          </button>
        </div>
      </div>
    </div>
  );
};

const SetupSection: Component<{ title: string; children: unknown }> = (props) => (
  <div class="decks-setup-section">
    <div class="decks-setup-section-title">{props.title}</div>
    <div class="decks-setup-section-body">
      {props.children as never}
    </div>
  </div>
);

export default SetupRail;
