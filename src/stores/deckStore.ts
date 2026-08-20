// Deck Builder store — signals + lifecycle (subscribes to backend deck:* events,
// hydrates status/templates on startup, refreshes commander candidates when
// the memory collection updates). Actions live in `deckStoreActions.ts` to
// keep this file focused and under the 500-line soft cap.

import { createSignal, type Accessor } from "solid-js";
import type { UnlistenFn } from "@tauri-apps/api/event";

import {
  Events,
  subscribe,
  type BuildResult,
  type CardDbReadyPayload,
  type CollectionScannedPayload,
  type CommanderCandidate,
  type CommanderCommunity,
  type DeckIngestCompletePayload,
  type DeckIngestErrorPayload,
  type DeckIngestProgressPayload,
  type DeckStatus,
  type Format,
  type OwnershipMode,
  type Playstyle,
  type SavedDeckRow,
  type Seed,
  type SeedPage,
  type SeedScore,
  type StartupCompletePayload,
  type Template,
  type TemplateOverrides,
} from "../lib/tauri";

// deckStoreActions is loaded lazily inside initDeckStore() to break the
// circular import — deckStoreActions destructures `_internal` at module top
// level, so it must load AFTER this module finishes evaluating.

// --- Setup / request signals ---

const [format, setFormat] = createSignal<Format>("brawl100");
const [ownership, setOwnership] = createSignal<OwnershipMode>({
  mode: "owned_only",
  wc_budget: { common: 0, uncommon: 0, rare: 0, mythic: 0 },
});
const [playstyle, setPlaystyle] = createSignal<Playstyle>("midrange");
const [colorSubset, setColorSubset] = createSignal<string | null>(null);
const [themeTags, setThemeTags] = createSignal<string[]>([]);
const [templateOverrides, setTemplateOverrides] =
  createSignal<TemplateOverrides | null>(null);
const [use17Lands, setUse17Lands] = createSignal<boolean>(false);
const [commanderGrp, setCommanderGrp] = createSignal<number | null>(null);
const [seed, setSeed] = createSignal<Seed | null>(null);
const [step, setStep] = createSignal<"pick" | "deck">("pick");

// --- Anchors (must-include / build-around / exclude) ---

interface Anchors {
  must: Set<number>;
  around: Map<number, number>;
  exclude: Set<number>;
}

const [anchors, setAnchors] = createSignal<Anchors>({
  must: new Set(),
  around: new Map(),
  exclude: new Set(),
});

function replaceAnchors(mutate: (a: Anchors) => Anchors): void {
  setAnchors(mutate({
    must: new Set(anchors().must),
    around: new Map(anchors().around),
    exclude: new Set(anchors().exclude),
  }));
}

function togglePin(grp: number): void {
  replaceAnchors((a) => {
    if (a.must.has(grp)) a.must.delete(grp);
    else {
      a.must.add(grp);
      a.exclude.delete(grp);
    }
    return a;
  });
}

function toggleStar(grp: number): void {
  replaceAnchors((a) => {
    if (a.around.has(grp)) a.around.delete(grp);
    else {
      a.around.set(grp, 1);
      a.exclude.delete(grp);
    }
    return a;
  });
}

function toggleBan(grp: number): void {
  replaceAnchors((a) => {
    if (a.exclude.has(grp)) a.exclude.delete(grp);
    else {
      a.exclude.add(grp);
      a.must.delete(grp);
      a.around.delete(grp);
    }
    return a;
  });
}

// --- Async / server-derived signals ---

const [status, setStatus] = createSignal<DeckStatus | null>(null);
const [ingest, setIngest] = createSignal<{
  running: boolean;
  stage: string;
  done: number;
  total: number;
  error: string | null;
}>({ running: false, stage: "", done: 0, total: 0, error: null });

const [candidates, setCandidates] = createSignal<CommanderCandidate[]>([]);
const [candidatesLoading, setCandidatesLoading] = createSignal<boolean>(false);

const [seedPage, setSeedPage] = createSignal<SeedPage | null>(null);
const [seedScores, setSeedScores] =
  createSignal<Record<number, SeedScore>>({});

const [community, setCommunity] = createSignal<CommanderCommunity | null>(null);

const [result, setResult] = createSignal<BuildResult | null>(null);
const [building, setBuilding] = createSignal<boolean>(false);
const [buildError, setBuildError] = createSignal<string | null>(null);

const [savedDecks, setSavedDecks] = createSignal<SavedDeckRow[]>([]);
const [templates, setTemplates] = createSignal<Template[]>([]);

// --- Public setter/accessor exports ---

export {
  format,
  setFormat,
  ownership,
  setOwnership,
  playstyle,
  setPlaystyle,
  colorSubset,
  setColorSubset,
  themeTags,
  setThemeTags,
  templateOverrides,
  setTemplateOverrides,
  use17Lands,
  setUse17Lands,
  commanderGrp,
  setCommanderGrp,
  seed,
  setSeed,
  step,
  setStep,
  anchors,
  togglePin,
  toggleStar,
  toggleBan,
  status,
  ingest,
  candidates,
  candidatesLoading,
  seedPage,
  seedScores,
  community,
  result,
  building,
  buildError,
  savedDecks,
  templates,
};

// Re-typed accessor exports for external declarations that documented them
// as `Accessor<T>`; runtime is identical to the createSignal getter.
export type StatusAccessor = Accessor<DeckStatus | null>;

// --- Internal setters used only by deckStoreActions ---

export const _internal = {
  setStatus,
  setIngest,
  setCandidates,
  setCandidatesLoading,
  setSeedPage,
  setSeedScores,
  setCommunity,
  setResult,
  setBuilding,
  setBuildError,
  setSavedDecks,
  setTemplates,
};

// --- Lifecycle ---

const unlisteners: UnlistenFn[] = [];

export async function initDeckStore(): Promise<void> {
  const { refreshCandidates, refreshStatus } = await import("./deckStoreActions");
  unlisteners.push(
    await subscribe<DeckIngestProgressPayload>(
      Events.deck.INGEST_PROGRESS,
      (p) =>
        setIngest({
          running: true,
          stage: p.stage,
          done: p.done,
          total: p.total,
          error: null,
        }),
    ),

    await subscribe<DeckIngestCompletePayload>(
      Events.deck.INGEST_COMPLETE,
      () => {
        setIngest({
          running: false,
          stage: "",
          done: 0,
          total: 0,
          error: null,
        });
        void refreshStatus();
        void refreshCandidates();
      },
    ),

    await subscribe<DeckIngestErrorPayload>(
      Events.deck.INGEST_ERROR,
      (p) =>
        setIngest({
          running: false,
          stage: "",
          done: 0,
          total: 0,
          error: p.message,
        }),
    ),

    await subscribe<StartupCompletePayload>(
      Events.app.STARTUP_COMPLETE,
      () => void refreshStatus(),
    ),

    await subscribe<CardDbReadyPayload>(
      Events.app.CARD_DB_READY,
      () => void refreshStatus(),
    ),

    await subscribe<CollectionScannedPayload>(
      Events.memory.COLLECTION_SCANNED,
      () => {
        // Refresh candidates so owned/buildability reflect the new collection.
        // Do NOT auto-rebuild — user presses Rebuild to accept new state.
        void refreshCandidates();
      },
    ),
  );

  // Hydrate whatever is already ready. Backend startup often emits
  // INGEST_COMPLETE / COLLECTION_SCANNED before the webview subscribes,
  // so we fetch candidates once here instead of relying only on events.
  await refreshStatus();
  void refreshCandidates();
}

export function cleanupDeckStore(): void {
  for (const unlisten of unlisteners) {
    unlisten();
  }
  unlisteners.length = 0;
}
