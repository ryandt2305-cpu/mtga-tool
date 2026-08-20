// DeckList — centre pane once a commander is picked and a BuildResult exists.
// Header: commander tile + action buttons.
// Groups: role sections in ROLE_ORDER with count / min–max header.
// Rows: 40px thumbnail, name → CardOverlay, cmc chip, ownership state,
// reason chips, action icons (pin / star / ban / ⇄).

import { type Component, For, Show, createEffect, createMemo, createSignal } from "solid-js";
import type { Role, Slot } from "../../lib/tauri";
import {
  anchors,
  building,
  buildError,
  commanderGrp,
  ownership,
  result,
  setStep,
  togglePin,
  toggleStar,
  toggleBan,
} from "../../stores/deckStore";
import {
  build,
  clearResult,
  copyForArena,
  fitToCollection,
  saveCurrent,
} from "../../stores/deckStoreActions";
import { deckExportArena } from "../../lib/tauri";
import { allCards, setOverlayCards } from "../../stores/collectionStore";
import { getCardImageUrl } from "../../stores/priceStore";
import AlternativesPopover, { wildcardIconPath } from "./AlternativesPopover";
import EdhrecStrip from "./EdhrecStrip";

const ROLE_ORDER: Role[] = [
  "land", "ramp", "draw", "removal", "wipe", "counter",
  "recursion", "tutor", "protection", "threat", "finisher", "payoff", "utility",
];

const ROLE_LABEL: Record<Role, string> = {
  land: "Lands",
  ramp: "Ramp",
  draw: "Card draw",
  removal: "Removal",
  wipe: "Wipes",
  counter: "Counterspells",
  recursion: "Recursion",
  tutor: "Tutors",
  protection: "Protection",
  threat: "Threats",
  finisher: "Finishers",
  payoff: "Payoffs",
  utility: "Utility",
};

// --- Root ---

const DeckList: Component = () => {
  const r = () => result();
  const [savingName, setSavingName] = createSignal<string | null>(null);
  const [showExport, setShowExport] = createSignal(false);
  const [exportText, setExportText] = createSignal("");
  const [openAltGrp, setOpenAltGrp] = createSignal<number | null>(null);

  // Track pending anchor changes since last build to drive the "changes pending" dot.
  const [dirty, setDirty] = createSignal(false);
  let lastAnchorSig = "";
  createEffect(() => {
    if (r() === null) return;
    // Snapshot anchors state at the moment a new result lands.
    lastAnchorSig = anchorSignature();
    setDirty(false);
  });
  createEffect(() => {
    // Runs whenever anchors() changes.
    const sig = anchorSignature();
    if (r() !== null && sig !== lastAnchorSig) setDirty(true);
  });

  function anchorSignature(): string {
    const a = anchors();
    return `${[...a.must].sort().join(",")}|${[...a.around.keys()].sort().join(",")}|${[...a.exclude].sort().join(",")}`;
  }

  async function onCopy(): Promise<void> {
    await copyForArena();
  }

  async function onExportText(): Promise<void> {
    const cur = r();
    if (cur === null) return;
    const text = await deckExportArena(cur);
    setExportText(text);
    setShowExport(true);
  }

  function onSaveSubmit(name: string): void {
    const trimmed = name.trim();
    if (trimmed.length === 0) { setSavingName(null); return; }
    void saveCurrent(trimmed);
    setSavingName(null);
  }

  const hasUnowned = createMemo(() => {
    const cur = r();
    return cur !== null && cur.slots.some((s) => s.owned_qty === 0);
  });
  const showFit = () => ownership().mode === "best_deck" || hasUnowned();

  const grouped = createMemo(() => {
    const cur = r();
    if (cur === null) return [] as { role: Role; slots: Slot[] }[];
    const map = new Map<Role, Slot[]>();
    for (const s of cur.slots) {
      const list = map.get(s.role) ?? [];
      list.push(s);
      map.set(s.role, list);
    }
    return ROLE_ORDER
      .map((role) => ({ role, slots: map.get(role) ?? [] }))
      .filter((g) => g.slots.length > 0);
  });

  return (
    <Show
      when={r() !== null}
      fallback={<DeckPlaceholder />}
    >
      <div class="decks-list">
        <HeaderBar
          onBack={() => { clearResult(); setStep("pick"); }}
          onRebuild={() => void build()}
          onFit={() => void fitToCollection()}
          onCopy={onCopy}
          onExport={onExportText}
          onSaveStart={() => setSavingName("")}
          savingName={savingName}
          onSaveSubmit={onSaveSubmit}
          onSaveCancel={() => setSavingName(null)}
          building={building}
          dirty={dirty}
          showFit={showFit}
        />
        <Show when={buildError() !== null}>
          <div class="decks-build-error">{buildError()}</div>
        </Show>
        <EdhrecStrip />
        <div class="decks-groups">
          <For each={grouped()}>
            {(g) => (
              <DeckGroup
                role={g.role}
                slots={g.slots}
                openAltGrp={openAltGrp}
                setOpenAltGrp={setOpenAltGrp}
              />
            )}
          </For>
        </div>
      </div>
      <Show when={showExport()}>
        <ExportModal text={exportText()} onClose={() => setShowExport(false)} />
      </Show>
    </Show>
  );
};

const DeckPlaceholder: Component = () => (
  <div class="decks-empty">
    <Show
      when={building()}
      fallback={<span>No deck yet — build one from the commander picker.</span>}
    >
      <span>Building…</span>
    </Show>
  </div>
);

// --- Header ---

interface HeaderBarProps {
  onBack: () => void;
  onRebuild: () => void;
  onFit: () => void;
  onCopy: () => Promise<void>;
  onExport: () => Promise<void>;
  onSaveStart: () => void;
  savingName: () => string | null;
  onSaveSubmit: (name: string) => void;
  onSaveCancel: () => void;
  building: () => boolean;
  dirty: () => boolean;
  showFit: () => boolean;
}

const HeaderBar: Component<HeaderBarProps> = (props) => {
  const cur = () => result()!;
  const cmd = createMemo(() => cur().commander);
  const card = createMemo(() => {
    const grp = commanderGrp();
    return grp !== null ? allCards()[grp] : undefined;
  });
  const identity = () => {
    const c = card();
    return c ? c.color_identity ?? [] : [];
  };
  const thumb = () => {
    const c = card();
    return c ? getCardImageUrl(c, "small") : "";
  };

  function openCmd(): void {
    const c = card();
    if (c !== undefined) setOverlayCards([c]);
  }

  return (
    <div class="decks-list-header">
      <div class="decks-cmd-summary" onClick={openCmd} title="Open commander">
        <Show
          when={card() !== undefined}
          fallback={<div class="decks-cmd-thumb decks-cmd-thumb--fallback">{cmd().name}</div>}
        >
          <img class="decks-cmd-thumb" src={thumb()} alt={cmd().name} />
        </Show>
        <div class="decks-cmd-meta">
          <div class="decks-cmd-title">{cmd().name}</div>
          <div class="decks-cmd-identity">
            <For each={identity()}>
              {(letter) => <span class={`decks-pip decks-pip--${letter}`} title={letter} />}
            </For>
          </div>
        </div>
      </div>
      <div class="decks-list-actions">
        <button class="decks-btn" onClick={props.onBack}>Back</button>
        <button
          class="decks-btn decks-btn--primary"
          onClick={props.onRebuild}
          disabled={props.building()}
        >
          <span>Rebuild</span>
          <Show when={props.dirty()}>
            <span class="decks-dirty-dot" title="Anchors changed since last build" />
          </Show>
        </button>
        <Show when={props.showFit()}>
          <button class="decks-btn" onClick={props.onFit} disabled={props.building()}>
            Fit to collection
          </button>
        </Show>
        <button class="decks-btn" onClick={() => void props.onCopy()}>Copy for Arena</button>
        <Show
          when={props.savingName() === null}
          fallback={
            <SaveInline
              initial={props.savingName() ?? ""}
              onSubmit={props.onSaveSubmit}
              onCancel={props.onSaveCancel}
            />
          }
        >
          <button class="decks-btn" onClick={props.onSaveStart}>Save…</button>
        </Show>
        <button class="decks-btn" onClick={() => void props.onExport()}>Export text</button>
      </div>
    </div>
  );
};

const SaveInline: Component<{
  initial: string;
  onSubmit: (name: string) => void;
  onCancel: () => void;
}> = (props) => {
  const [val, setVal] = createSignal(props.initial);
  let inputEl: HTMLInputElement | undefined;
  setTimeout(() => inputEl?.focus(), 0);
  return (
    <span class="decks-save-inline">
      <input
        ref={inputEl}
        class="decks-save-input"
        placeholder="Deck name"
        value={val()}
        onInput={(e) => setVal((e.currentTarget as HTMLInputElement).value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") props.onSubmit(val());
          if (e.key === "Escape") props.onCancel();
        }}
      />
      <button class="decks-btn decks-btn--primary" onClick={() => props.onSubmit(val())}>
        Save
      </button>
      <button class="decks-btn" onClick={props.onCancel}>Cancel</button>
    </span>
  );
};

// --- Groups + rows ---

interface DeckGroupProps {
  role: Role;
  slots: Slot[];
  openAltGrp: () => number | null;
  setOpenAltGrp: (v: number | null) => void;
}

const DeckGroup: Component<DeckGroupProps> = (props) => {
  const count = () => props.slots.reduce((n, s) => n + Math.max(1, s.needed_qty), 0);
  const range = createMemo(() => {
    const cur = result();
    if (cur === null) return null;
    const t = cur.stats.role_targets[props.role];
    return t ?? null;
  });
  return (
    <section class="decks-group">
      <header class="decks-group-head">
        <span class="decks-group-label">{ROLE_LABEL[props.role]}</span>
        <span class="decks-group-count">
          {count()}
          <Show when={range() !== null}>
            <span class="decks-group-range">{" "} / {range()![0]}–{range()![1]}</span>
          </Show>
        </span>
      </header>
      <ul class="decks-slot-list">
        <For each={props.slots}>
          {(slot) => (
            <DeckRow
              slot={slot}
              openAltGrp={props.openAltGrp}
              setOpenAltGrp={props.setOpenAltGrp}
            />
          )}
        </For>
      </ul>
    </section>
  );
};

const DeckRow: Component<{
  slot: Slot;
  openAltGrp: () => number | null;
  setOpenAltGrp: (v: number | null) => void;
}> = (props) => {
  const card = createMemo(() => allCards()[props.slot.grp_id]);
  const thumb = () => {
    const c = card();
    return c ? getCardImageUrl(c, "small") : "";
  };
  const owned = () => props.slot.owned_qty > 0;
  const wc = () => wildcardIconPath(props.slot.wildcard_rarity);
  const a = anchors;
  const pinned = () => a().must.has(props.slot.grp_id);
  const starred = () => a().around.has(props.slot.grp_id);
  const banned = () => a().exclude.has(props.slot.grp_id);
  const altsOpen = () => props.openAltGrp() === props.slot.grp_id;

  function openCard(): void {
    const c = card();
    if (c !== undefined) setOverlayCards([c]);
  }

  function toggleAlts(e: MouseEvent): void {
    e.stopPropagation();
    props.setOpenAltGrp(altsOpen() ? null : props.slot.grp_id);
  }

  return (
    <li class={`decks-slot ${owned() ? "" : "decks-slot--unowned"} ${banned() ? "decks-slot--banned" : ""}`}>
      <Show
        when={card() !== undefined}
        fallback={<div class="decks-slot-thumb decks-slot-thumb--fallback">{props.slot.name}</div>}
      >
        <img
          class="decks-slot-thumb"
          src={thumb()}
          alt={props.slot.name}
          onError={(e) => {
            const img = e.currentTarget;
            const fallback = document.createElement("div");
            fallback.className = "decks-slot-thumb decks-slot-thumb--fallback";
            fallback.textContent = props.slot.name;
            img.replaceWith(fallback);
          }}
        />
      </Show>
      <span class="decks-slot-name" onClick={openCard} title="Open card">
        {props.slot.name}
      </span>
      <span class="decks-slot-cmc">{props.slot.cmc}</span>
      <span class="decks-slot-own">
        <Show when={!owned() && wc() !== null}>
          <img class="decks-wc-icon" src={wc()!} alt="wildcard" />
        </Show>
      </span>
      <span class="decks-slot-reasons">
        <For each={props.slot.reasons.slice(0, 3)}>
          {(reason) => <span class="decks-reason-chip">{reason}</span>}
        </For>
      </span>
      <span class="decks-slot-actions">
        <button
          class={`decks-icon-btn ${pinned() ? "decks-icon-btn--on" : ""}`}
          onClick={() => togglePin(props.slot.grp_id)}
          title="Pin (must include)"
        >📌</button>
        <button
          class={`decks-icon-btn ${starred() ? "decks-icon-btn--on" : ""}`}
          onClick={() => toggleStar(props.slot.grp_id)}
          title="Star (build around)"
        >★</button>
        <button
          class={`decks-icon-btn ${banned() ? "decks-icon-btn--on" : ""}`}
          onClick={() => toggleBan(props.slot.grp_id)}
          title="Ban (exclude)"
        >⛔</button>
        <button
          class={`decks-icon-btn ${altsOpen() ? "decks-icon-btn--on" : ""}`}
          onClick={toggleAlts}
          title="Alternatives"
        >⇄</button>
        <Show when={altsOpen()}>
          <AlternativesPopover
            slot={props.slot}
            onClose={() => props.setOpenAltGrp(null)}
          />
        </Show>
      </span>
    </li>
  );
};

// --- Export modal ---

const ExportModal: Component<{ text: string; onClose: () => void }> = (props) => (
  <div class="decks-modal-backdrop" onClick={props.onClose}>
    <div class="decks-modal" onClick={(e) => e.stopPropagation()}>
      <div class="decks-modal-head">
        <span>Deck text (Arena format)</span>
        <button class="decks-alts-close" onClick={props.onClose} aria-label="Close">×</button>
      </div>
      <textarea class="decks-modal-text" readOnly>{props.text}</textarea>
    </div>
  </div>
);

export default DeckList;
