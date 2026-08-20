// Deck Builder actions — assemble BuildRequest from store signals and issue
// Tauri commands, coalescing rapid refreshCandidates calls behind a 150ms
// debounce. Signals live in `deckStore.ts`; only that module and this one
// mutate them.

import {
  deckBuild,
  deckDelete,
  deckExportArena,
  deckGetCommanderCandidates,
  deckGetCommanderCommunity,
  deckGetSeedDecks,
  deckGetStatus,
  deckGetTemplates,
  deckList,
  deckPrefetchCommunity,
  deckRefreshBulk,
  deckRescore,
  deckSave,
  deckScoreSeed,
  type BuildRequest,
  type CandidateRequest,
} from "../lib/tauri";

import {
  _internal,
  anchors,
  colorSubset,
  commanderGrp,
  format,
  ownership,
  playstyle,
  result,
  seed,
  seedScores,
  templates,
  themeTags,
  templateOverrides,
  use17Lands,
} from "./deckStore";

const {
  setStatus,
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
} = _internal;

// --- State transitions ---

/** Clear the last build result. Called when the user changes commander,
 *  otherwise DeckList would show a stale deck from the previous commander
 *  while the new build runs. */
export function clearResult(): void {
  setResult(null);
  setBuildError(null);
}

/** Warm the community-source cache in the background so a subsequent build
 *  doesn't pay the cold-fetch cost. Fire-and-forget; errors are swallowed
 *  because the real build call will report them if the sources are still
 *  unavailable when it runs. */
export function prefetchCommunity(): void {
  const req = currentRequest();
  if (req.commander_grp === null) return;
  void deckPrefetchCommunity(req).catch(() => {
    // Prefetch is best-effort; build() reports real errors.
  });
}

// --- Request assembly ---

export function currentRequest(): BuildRequest {
  const a = anchors();
  return {
    format: format(),
    commander_grp: commanderGrp(),
    ownership: ownership(),
    playstyle: playstyle(),
    color_subset: colorSubset(),
    theme_tags: themeTags(),
    must_include: Array.from(a.must),
    build_around: Array.from(a.around.entries()).map(([grp_id, weight]) => ({
      grp_id,
      weight,
    })),
    exclude: Array.from(a.exclude),
    seed: seed(),
    template_overrides: templateOverrides(),
    use_17lands: use17Lands(),
  };
}

// --- Status / templates ---

export async function refreshStatus(): Promise<void> {
  try {
    const status = await deckGetStatus();
    setStatus(status);
    // Templates load once per session — cheap and static.
    if (templates().length === 0) {
      try {
        const ts = await deckGetTemplates();
        setTemplates(ts);
      } catch {
        // Templates not critical; a later refresh will retry.
      }
    }
  } catch {
    // Backend not ready — will retry on the next deck:* / app:* event.
  }
}

// --- Commander candidates (debounced) ---

let candidatesTimer: number | null = null;
let candidatesSeq = 0;

export function refreshCandidates(search?: string): void {
  if (candidatesTimer !== null) {
    clearTimeout(candidatesTimer);
  }
  const searchArg = search ?? null;
  candidatesTimer = window.setTimeout(() => {
    candidatesTimer = null;
    void doRefreshCandidates(searchArg);
  }, 150);
}

async function doRefreshCandidates(search: string | null): Promise<void> {
  const req: CandidateRequest = {
    format: format(),
    owned_only: ownership().mode === "owned_only",
    color_subset: colorSubset(),
    theme_tags: themeTags(),
    search,
    playstyle: playstyle(),
  };
  const seq = ++candidatesSeq;
  setCandidatesLoading(true);
  try {
    const rows = await deckGetCommanderCandidates(req, null);
    if (seq === candidatesSeq) {
      setCandidates(rows);
    }
  } catch {
    if (seq === candidatesSeq) {
      setCandidates([]);
    }
  } finally {
    if (seq === candidatesSeq) {
      setCandidatesLoading(false);
    }
  }
}

// --- Seed decks ---

export async function loadSeedPage(page: number): Promise<void> {
  try {
    const p = await deckGetSeedDecks(format(), page);
    setSeedPage(p);
  } catch {
    setSeedPage(null);
  }
}

export async function scoreSeed(id: number): Promise<void> {
  try {
    const score = await deckScoreSeed(id);
    setSeedScores({ ...seedScores(), [id]: score });
  } catch {
    // Score failure is per-row; UI just leaves the cell blank.
  }
}

// --- Community ---

export async function loadCommunity(): Promise<void> {
  const grp = commanderGrp();
  if (grp === null) {
    setCommunity(null);
    return;
  }
  try {
    const c = await deckGetCommanderCommunity(grp);
    setCommunity(c);
  } catch {
    setCommunity({ edhrec: [], combos: [], available: false });
  }
}

// --- Build / rescore / fit ---

export async function build(): Promise<void> {
  setBuilding(true);
  setBuildError(null);
  try {
    const r = await deckBuild(currentRequest());
    setResult(r);
  } catch (err) {
    setBuildError(errorMessage(err));
  } finally {
    setBuilding(false);
  }
}

export async function rescoreKeeping(keepGrp: number[]): Promise<void> {
  setBuilding(true);
  setBuildError(null);
  try {
    const r = await deckRescore(currentRequest(), keepGrp);
    setResult(r);
  } catch (err) {
    setBuildError(errorMessage(err));
  } finally {
    setBuilding(false);
  }
}

export async function fitToCollection(): Promise<void> {
  const r = result();
  if (r === null) return;
  const owned = r.slots.filter((s) => s.owned_qty > 0).map((s) => s.grp_id);
  // Force owned_only for the fit pass; the user's ownership signal is left
  // untouched — this is a one-shot rescore.
  const req = currentRequest();
  req.ownership = {
    mode: "owned_only",
    wc_budget: { common: 0, uncommon: 0, rare: 0, mythic: 0 },
  };
  setBuilding(true);
  setBuildError(null);
  try {
    const next = await deckRescore(req, owned);
    setResult(next);
  } catch (err) {
    setBuildError(errorMessage(err));
  } finally {
    setBuilding(false);
  }
}

// --- Save / load ---

export async function saveCurrent(name: string): Promise<void> {
  const r = result();
  if (r === null) return;
  const grp = commanderGrp();
  if (grp === null) return;
  const input = {
    id: null,
    name,
    format: format(),
    commander_grp: grp,
    request_json: JSON.stringify(currentRequest()),
    result_json: JSON.stringify(r),
  };
  try {
    await deckSave(input);
    await refreshSavedDecks();
  } catch (err) {
    setBuildError(errorMessage(err));
  }
}

export async function loadSaved(id: number): Promise<void> {
  try {
    const rows = await deckList();
    const row = rows.find((r) => r.id === id);
    if (!row) return;
    const req = JSON.parse(row.request_json) as BuildRequest;
    const res = JSON.parse(row.result_json);
    // Apply request fields back into store via re-imported setters would
    // create a cycle here — we drive the UI by setting the result signal
    // and letting the user rebuild if they want the request reflected.
    setResult(res);
    void req;
  } catch (err) {
    setBuildError(errorMessage(err));
  }
}

export async function deleteSaved(id: number): Promise<void> {
  try {
    await deckDelete(id);
    await refreshSavedDecks();
  } catch (err) {
    setBuildError(errorMessage(err));
  }
}

export async function refreshSavedDecks(): Promise<void> {
  try {
    const rows = await deckList();
    setSavedDecks(rows);
  } catch {
    // Non-fatal.
  }
}

// --- Copy for Arena / refresh bulk ---

export async function copyForArena(): Promise<string> {
  const r = result();
  if (r === null) return "";
  const text = await deckExportArena(r);
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    // Clipboard may be denied in some contexts; the returned string lets the
    // caller show it in a fallback textarea.
  }
  return text;
}

export async function refreshBulk(): Promise<void> {
  try {
    await deckRefreshBulk();
    await refreshStatus();
  } catch (err) {
    setBuildError(errorMessage(err));
  }
}

// --- Helpers ---

function errorMessage(err: unknown): string {
  if (err instanceof Error) return err.message;
  if (typeof err === "string") return err;
  return JSON.stringify(err);
}
