// Deck Builder types — mirror Rust structs in src-tauri/src/services/deck/**.
// Kept out of tauri.ts to respect the 750-line hard limit; re-exported from it.

export type Role =
  | "land"
  | "ramp"
  | "draw"
  | "removal"
  | "wipe"
  | "counter"
  | "recursion"
  | "tutor"
  | "protection"
  | "threat"
  | "finisher"
  | "payoff"
  | "utility";

export type Format = "brawl100" | "standardbrawl60";
export type Playstyle = "aggro" | "midrange" | "control";

export interface WildcardBudget {
  common: number;
  uncommon: number;
  rare: number;
  mythic: number;
}

// Serde tagged enums: `#[serde(tag = "mode"/"kind", rename_all = "snake_case")]`.
export type OwnershipMode =
  | { mode: "owned_only"; wc_budget: WildcardBudget }
  | { mode: "best_deck" };

export type Seed =
  | { kind: "edhrec" }
  | { kind: "archidekt"; deck_id: number };

// Rarity — `#[serde(rename_all = "snake_case")]` yields snake_case for unit
// variants and a tagged object for `Unknown(i32)`.
export type Rarity =
  | "special"
  | "basic_land"
  | "common"
  | "uncommon"
  | "rare"
  | "mythic"
  | { unknown: number };

export interface TemplateOverrides {
  lands?: number | null;
  role_min?: Partial<Record<Role, number>>;
  role_max?: Partial<Record<Role, number>>;
}

export interface BuildAround {
  grp_id: number;
  weight: number;
}

export interface BuildRequest {
  format: Format;
  commander_grp: number | null;
  ownership: OwnershipMode;
  playstyle: Playstyle;
  color_subset?: string | null;
  theme_tags?: string[];
  must_include?: number[];
  build_around?: BuildAround[];
  exclude?: number[];
  seed?: Seed | null;
  template_overrides?: TemplateOverrides | null;
  use_17lands?: boolean;
}

export interface Alternative {
  grp_id: number;
  name: string;
  owned: boolean;
  wildcard_rarity: Rarity | null;
  score: number;
  reasons: string[];
}

export interface Slot {
  grp_id: number;
  oracle_id: string;
  name: string;
  role: Role;
  cmc: number;
  owned_qty: number;
  needed_qty: number;
  wildcard_rarity: Rarity | null;
  score: number;
  reasons: string[];
  anchored: boolean;
  alternatives: Alternative[];
}

export interface DeckStats {
  curve: number[];         // fixed 8
  pips: number[];          // fixed 5 [W,U,B,R,G]
  role_counts: Partial<Record<Role, number>>;
  role_targets: Partial<Record<Role, [number, number]>>;
  owned: number;
  total: number;
  wildcard_cost: WildcardBudget;
  land_count: number;
}

export interface Warning {
  code: string;
  message: string;
}

export interface BuildResult {
  format: Format;
  commander: Slot;
  slots: Slot[];
  stats: DeckStats;
  warnings: Warning[];
}

export interface Weights {
  role: number;
  synergy: number;
  power: number;
  ownership: number;
  curve: number;
  pip: number;
}

export interface Template {
  format: Format;
  playstyle: Playstyle;
  deck_size: number;
  lands: number;
  role_min: Partial<Record<Role, number>>;
  role_max: Partial<Record<Role, number>>;
  curve: number[];         // 8
  weights: Weights;
  nonbasic_land_cap: number;
}

export interface CandidateRequest {
  format: Format;
  owned_only?: boolean;
  color_subset?: string | null;
  theme_tags?: string[];
  search?: string | null;
  playstyle: Playstyle;
}

export interface CommanderCandidate {
  grp_id: number;
  oracle_id: string;
  name: string;
  color_identity: string;
  owned: boolean;
  buildability: number;
  popularity: number;
  theme_match: number;
  score: number;
  tags: string[];
}

export interface SeedDeckSummary {
  id: number;
  name: string;
  size: number;
  view_count: number;
  colors: string[];
}

// Backend flattens SeedDeckSummary into SeedDeckSummaryScored, so the extra
// scoring fields sit alongside the base ones.
export interface SeedDeckSummaryScored extends SeedDeckSummary {
  owned_pct: number | null;
  wildcard_cost: WildcardBudget | null;
  unmatched: number | null;
}

export interface SeedPage {
  decks: SeedDeckSummaryScored[];
  has_next: boolean;
  stale: boolean;
}

export interface SeedDeckDetail {
  id: number;
  name: string;
  commander_names: string[];
  cards: Array<[string, number]>;
}

export interface SeedScore {
  owned_pct: number;
  wildcard_cost: WildcardBudget;
  unmatched: number;
}

export interface EdhrecCard {
  name: string;
  inclusion: number;
  synergy: number;
  section: string;
}

export interface CommanderCommunity {
  edhrec: EdhrecCard[];
  combos: string[][];
  available: boolean;
}

export interface SavedDeckInput {
  id: number | null;
  name: string;
  format: string;
  commander_grp: number;
  request_json: string;
  result_json: string;
}

export interface SavedDeckRow {
  id: number;
  name: string;
  format: string;
  commander_grp: number;
  request_json: string;
  result_json: string;
  created_at: number;
  updated_at: number;
}

export interface DeckStatus {
  db_ok: boolean;
  oracles: number;
  bulk_updated_at: string | null;
  last_check_at: number | null;
  ingest_running: boolean;
  degraded_hosts: string[];
}

// --- Event payloads ---

export interface DeckIngestProgressPayload {
  stage: string;
  done: number;
  total: number;
}

export interface DeckIngestCompletePayload {
  changed: boolean;
  oracles: number;
  unresolved_grp: number;
}

export interface DeckIngestErrorPayload {
  code: string;
  message: string;
}

export interface DeckSourcesUpdatedPayload {
  host: string;
  key: string;
}
