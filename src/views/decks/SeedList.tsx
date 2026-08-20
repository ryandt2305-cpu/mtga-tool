// SeedList — "Seed decks" tab of the Commander picker (T18).
// Fetches Archidekt-sourced Brawl decks page-by-page (accumulating locally
// so "Load more" appends), lazily scores each visible row (owned% +
// wildcard cost) via IntersectionObserver, and on "Build from this"
// resolves the seed deck's commander to a grp, seeds the request, and
// triggers a build.

import {
  type Component,
  For,
  Show,
  createSignal,
  onCleanup,
  onMount,
} from "solid-js";
import {
  deckGetSeedDeckDetail,
  deckGetSeedDecks,
  type SeedDeckSummaryScored,
  type WildcardBudget,
} from "../../lib/tauri";
import {
  commanderGrp,
  format,
  seedScores,
  setCommanderGrp,
  setSeed,
  setStep,
} from "../../stores/deckStore";
import { cardsByName } from "../../stores/collectionStore";
import {
  build,
  clearResult,
  loadCommunity,
  scoreSeed,
} from "../../stores/deckStoreActions";

const WC_ICONS: Record<keyof WildcardBudget, string> = {
  common: "/icons/ObjectiveIcon_Wildcard_Common.png",
  uncommon: "/icons/ObjectiveIcon_Wildcard_Uncommon.png",
  rare: "/icons/ObjectiveIcon_Wildcard_Rare.png",
  mythic: "/icons/ObjectiveIcon_Wildcard_MythicRare.png",
};

const SeedList: Component = () => {
  const [decks, setDecks] = createSignal<SeedDeckSummaryScored[]>([]);
  const [page, setPage] = createSignal(0);
  const [hasNext, setHasNext] = createSignal(false);
  const [stale, setStale] = createSignal(false);
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [buildingFrom, setBuildingFrom] = createSignal<number | null>(null);

  async function fetchPage(n: number): Promise<void> {
    setLoading(true);
    setError(null);
    try {
      const p = await deckGetSeedDecks(format(), n);
      if (n === 1) setDecks(p.decks);
      else setDecks([...decks(), ...p.decks]);
      setPage(n);
      setHasNext(p.has_next);
      setStale(p.stale);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }

  onMount(() => {
    if (decks().length === 0) void fetchPage(1);
  });

  async function buildFromSeed(id: number): Promise<void> {
    setBuildingFrom(id);
    setError(null);
    try {
      const detail = await deckGetSeedDeckDetail(id);
      const cmdrName = detail.commander_names[0];
      if (!cmdrName) {
        setError("Seed deck has no commander.");
        return;
      }
      const matches = cardsByName().get(cmdrName);
      const grp = matches?.[0]?.grp_id ?? null;
      if (grp === null) {
        setError(`Commander "${cmdrName}" is not on Arena.`);
        return;
      }
      if (commanderGrp() !== grp) clearResult();
      setCommanderGrp(grp);
      setSeed({ kind: "archidekt", deck_id: id });
      setStep("deck");
      void loadCommunity();
      void build();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBuildingFrom(null);
    }
  }

  return (
    <div class="decks-seeds">
      <Show when={stale()}>
        <div class="decks-seeds-stale">
          Showing cached results (source unreachable).
        </div>
      </Show>
      <Show when={error() !== null}>
        <div class="decks-seeds-error">{error()}</div>
      </Show>
      <Show
        when={decks().length > 0}
        fallback={
          <div class="decks-empty">
            {loading() ? "Loading seed decks…" : "No seed decks available."}
          </div>
        }
      >
        <div class="decks-seed-rows">
          <For each={decks()}>
            {(deck) => (
              <SeedRow
                deck={deck}
                onBuild={() => void buildFromSeed(deck.id)}
                busy={buildingFrom() === deck.id}
              />
            )}
          </For>
        </div>
        <Show when={hasNext()}>
          <button
            class="decks-seeds-more"
            onClick={() => void fetchPage(page() + 1)}
            disabled={loading()}
          >
            {loading() ? "Loading…" : "Load more"}
          </button>
        </Show>
      </Show>
    </div>
  );
};

const SeedRow: Component<{
  deck: SeedDeckSummaryScored;
  onBuild: () => void;
  busy: boolean;
}> = (props) => {
  let rowRef: HTMLDivElement | undefined;

  onMount(() => {
    if (!rowRef) return;
    const scrollRoot = rowRef.closest(".view-content");
    const io = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting) {
          if (seedScores()[props.deck.id] === undefined) {
            void scoreSeed(props.deck.id);
          }
          io.disconnect();
        }
      },
      { root: scrollRoot, rootMargin: "50px" },
    );
    io.observe(rowRef);
    onCleanup(() => io.disconnect());
  });

  const score = () => seedScores()[props.deck.id];
  const wcKeys = () =>
    (Object.keys(WC_ICONS) as Array<keyof WildcardBudget>).filter(
      (k) => (score()?.wildcard_cost[k] ?? 0) > 0,
    );
  const wcZero = () =>
    score() !== undefined &&
    Object.values(score()!.wildcard_cost).every((v) => v === 0);

  return (
    <div class="decks-seed-row" ref={rowRef}>
      <div class="decks-seed-row-main">
        <div class="decks-seed-row-name">{props.deck.name}</div>
        <div class="decks-seed-row-meta">
          <span>{props.deck.size} cards</span>
          <span> · {props.deck.view_count.toLocaleString()} views</span>
          <Show when={props.deck.colors.length > 0}>
            <span class="decks-seed-row-colors">
              <For each={props.deck.colors}>
                {(c) => <span class={`decks-pip decks-pip--${c}`} />}
              </For>
            </span>
          </Show>
        </div>
        <div class="decks-seed-row-score">
          <Show
            when={score() !== undefined}
            fallback={<span class="decks-seed-row-scoring">Scoring…</span>}
          >
            <span>Owned {Math.round(score()!.owned_pct * 100)}%</span>
            <span class="decks-seed-row-wc">
              {" · cost "}
              <Show when={wcZero()}>
                <span>none</span>
              </Show>
              <For each={wcKeys()}>
                {(k) => (
                  <span class="decks-seed-row-wc-chip">
                    <img src={WC_ICONS[k]} alt={k} width="14" height="14" />
                    {score()!.wildcard_cost[k]}
                  </span>
                )}
              </For>
            </span>
            <Show when={score()!.unmatched > 0}>
              <span class="decks-seed-row-unmatched">
                {" · "}{score()!.unmatched} not on Arena
              </span>
            </Show>
          </Show>
        </div>
      </div>
      <button
        class="decks-seed-row-btn"
        onClick={props.onBuild}
        disabled={props.busy}
      >
        {props.busy ? "Building…" : "Build from this"}
      </button>
    </div>
  );
};

export default SeedList;
