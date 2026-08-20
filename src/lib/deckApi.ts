// Deck Builder command wrappers. Kept out of tauri.ts to respect the 750-line
// hard limit. Only `lib/tauri.ts` and `lib/deckApi.ts` may import
// `@tauri-apps/api` directly.

import { invoke } from "@tauri-apps/api/core";

import type {
  BuildRequest,
  BuildResult,
  CandidateRequest,
  CommanderCandidate,
  CommanderCommunity,
  DeckStatus,
  Format,
  SavedDeckInput,
  SavedDeckRow,
  SeedDeckDetail,
  SeedPage,
  SeedScore,
  Template,
} from "./deckTypes";

export function deckGetStatus(): Promise<DeckStatus> {
  return invoke<DeckStatus>("deck_get_status");
}

export function deckRefreshBulk(): Promise<boolean> {
  return invoke<boolean>("deck_refresh_bulk");
}

export function deckGetTemplates(): Promise<Template[]> {
  return invoke<Template[]>("deck_get_templates");
}

export function deckGetCommanderCandidates(
  req: CandidateRequest,
  limit?: number | null,
): Promise<CommanderCandidate[]> {
  return invoke<CommanderCandidate[]>("deck_get_commander_candidates", {
    req,
    limit: limit ?? null,
  });
}

export function deckGetSeedDecks(format: Format, page: number): Promise<SeedPage> {
  return invoke<SeedPage>("deck_get_seed_decks", { format, page });
}

export function deckGetSeedDeckDetail(deckId: number): Promise<SeedDeckDetail> {
  return invoke<SeedDeckDetail>("deck_get_seed_deck_detail", { deckId });
}

export function deckScoreSeed(deckId: number): Promise<SeedScore> {
  return invoke<SeedScore>("deck_score_seed", { deckId });
}

export function deckGetCommanderCommunity(commanderGrp: number): Promise<CommanderCommunity> {
  return invoke<CommanderCommunity>("deck_get_commander_community", { commanderGrp });
}

export function deckBuild(req: BuildRequest): Promise<BuildResult> {
  return invoke<BuildResult>("deck_build", { req });
}

/** Warm the community-source cache without running the engine. Frontend fires
 *  this the moment a commander is picked so the four hosts are all hot by the
 *  time the user hits Build. */
export function deckPrefetchCommunity(req: BuildRequest): Promise<void> {
  return invoke<void>("deck_prefetch_community", { req });
}

export function deckRescore(req: BuildRequest, keepGrp: number[]): Promise<BuildResult> {
  return invoke<BuildResult>("deck_rescore", { req, keepGrp });
}

export function deckSave(input: SavedDeckInput): Promise<number> {
  return invoke<number>("deck_save", { input });
}

export function deckList(): Promise<SavedDeckRow[]> {
  return invoke<SavedDeckRow[]>("deck_list");
}

export function deckDelete(id: number): Promise<void> {
  return invoke<void>("deck_delete", { id });
}

export function deckExportArena(result: BuildResult): Promise<string> {
  return invoke<string>("deck_export_arena", { result });
}
