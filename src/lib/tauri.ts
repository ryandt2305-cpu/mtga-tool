// Typed wrappers for Tauri backend. All frontend code imports from here or
// from `./deckApi` — never from `@tauri-apps/api` directly.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// --- Event name constants (mirror src-tauri/src/events.rs) ---

export const Events = {
  app: {
    STARTUP_COMPLETE: "app:startup_complete",
    CARD_DB_READY: "app:card_db_ready",
    LOG_READY: "app:log_ready",
    SCAN_STARTING: "app:scan_starting",
  },
  cardDb: {
    LOADED: "card_db:loaded",
    ERROR: "card_db:error",
  },
  diagnostics: {
    LOGGED: "diagnostics:logged",
  },
  schemaGuard: {
    WARNING: "schema_guard:warning",
  },
  memory: {
    PROCESS_FOUND: "memory:process_found",
    SCAN_STARTED: "memory:scan_started",
    SCAN_PROGRESS: "memory:scan_progress",
    COLLECTION_SCANNED: "memory:collection_scanned",
    SCAN_ERROR: "memory:scan_error",
    COLLECTION_CHANGED: "memory:collection_changed",
    WATCH_STARTED: "memory:watch_started",
    WATCH_STOPPED: "memory:watch_stopped",
    INVENTORY_SCANNED: "memory:inventory_scanned",
    INVENTORY_CHANGED: "memory:inventory_changed",
    OFFLINE_HYDRATED: "memory:offline_hydrated",
  },
  log: {
    WATCHER_STARTED: "log:watcher_started",
    WATCHER_STOPPED: "log:watcher_stopped",
    SESSION_STARTED: "log:session_started",
    INVENTORY_UPDATED: "log:inventory_updated",
    CARDS_GRANTED: "log:cards_granted",
    LOG_EVENT: "log:event",
    EVENTS_UPDATED: "log:events_updated",
    COSMETICS_UPDATED: "log:cosmetics_updated",
    MASTERY_UPDATED: "log:mastery_updated",
    MATCH_STARTING: "log:match_starting",
    MATCH_RESULT: "log:match_result",
    ACHIEVEMENTS_UPDATED: "log:achievements_updated",
  },
  history: {
    SNAPSHOT_SAVED: "history:snapshot_saved",
  },
  imageCache: {
    PROGRESS: "image_cache:progress",
    COMPLETE: "image_cache:complete",
  },
  deck: {
    INGEST_PROGRESS: "deck:ingest_progress",
    INGEST_COMPLETE: "deck:ingest_complete",
    INGEST_ERROR: "deck:ingest_error",
    SOURCES_UPDATED: "deck:sources_updated",
  },
} as const;

// Deck Builder types and command wrappers live in sibling modules to keep this
// file under the 750-line hard limit; re-exported here so callers can pull them
// from either module.
export * from "./deckTypes";
export * from "./deckApi";

// --- Payload interfaces (match Rust #[derive(Serialize)] structs) ---

export interface StartupCompletePayload {
  has_collection: boolean;
  has_inventory: boolean;
}

export interface CardDbReadyPayload {
  card_count: number;
}

export interface LogReadyPayload {
  has_inventory: boolean;
}

export interface ScanStartingPayload {}

export interface ScanStartedPayload {}

export interface ScanErrorPayload {
  message: string;
}

export interface CardDbLoadedPayload {
  card_count: number;
  db_path: string;
}

export interface CardDbErrorPayload {
  code: string;
  message: string;
}

export interface DiagnosticPayload {
  code: string;
  level: string;
  message: string;
}

export interface SchemaWarningPayload {
  code: string;
  message: string;
  details: string[];
}

export interface ProcessFoundPayload {
  pid: number;
}

export interface ScanProgressPayload {
  current_region: number;
  total_regions: number;
}

export interface CollectionScannedPayload {
  unique_cards: number;
  total_copies: number;
  candidates_found: number;
  scan_duration_ms: number;
}

export interface MemoryCollectionChangedPayload {
  added: [number, number][];
  increased: [number, number, number][];
  removed: [number, number][];
  scan_duration_ms: number;
}

export interface MemoryWatchStartedPayload {
  poll_interval_ms: number;
}

export interface MemoryWatchStoppedPayload {
  reason: string;
}

export interface OfflineHydratedPayload {
  snapshot_timestamp: number;
  unique_cards: number;
  total_copies: number;
}

// --- Event subscription ---

export function subscribe<T>(
  event: string,
  handler: (payload: T) => void,
): Promise<UnlistenFn> {
  return listen<T>(event, (e) => handler(e.payload));
}

// --- Command wrappers (match src-tauri/src/commands.rs) ---

export interface AppInfo {
  name: string;
  version: string;
}

export interface CardInfo {
  grp_id: number;
  name: string;
  set_code: string;
  rarity: string;
  collector_number: string;
  cmc: number;
  power: number | null;
  toughness: number | null;
  deck_limit: number;
  color_identity: string[];
  colors: string[];
  types: string[];
  subtypes: string[];
  supertypes: string[];
  is_rebalanced: boolean;
  rebalanced_grp_id: number | null;
  is_primary_card: boolean;
}

export interface AppStatus {
  card_db_loaded: boolean;
  card_db_card_count: number;
  card_db_path: string | null;
  mtga_pid: number | null;
  log_watcher_running: boolean;
  log_has_inventory: boolean;
  has_collection: boolean;
  collection_unique: number;
  collection_total: number;
  is_watching: boolean;
  is_scanning: boolean;
}

export function getAppInfo(): Promise<AppInfo> {
  return invoke<AppInfo>("get_app_info");
}

export function getStatus(): Promise<AppStatus> {
  return invoke<AppStatus>("get_status");
}

export function loadCardDb(pathOverride?: string): Promise<number> {
  return invoke<number>("load_card_db", { pathOverride });
}

export function getCard(grpId: number): Promise<CardInfo | null> {
  return invoke<CardInfo | null>("get_card", { grpId });
}

export interface CrossValidation {
  block_a_unique: number;
  block_b_unique: number;
  shared_ids: number;
  quantity_matches: number;
  quantity_mismatches: number;
  only_in_a: number;
  only_in_b: number;
}

export interface ScanResult {
  collection: Record<number, number>;
  candidate_count: number;
  cross_validation: CrossValidation | null;
  scan_duration_ms: number;
}

export function scanCollection(): Promise<void> {
  return invoke<void>("scan_collection");
}

export function cancelScan(): Promise<void> {
  return invoke<void>("cancel_scan");
}

// --- LogService types and commands ---

export interface BoosterStack {
  collation_id: number;
  count: number;
  set_code: string | null;
}

export interface PlayerInventory {
  gold: number;
  gems: number;
  vault_progress: number;
  wc_common: number;
  wc_uncommon: number;
  wc_rare: number;
  wc_mythic: number;
  wc_track_position: number;
  draft_tokens: number;
  sealed_tokens: number;
  boosters: BoosterStack[];
  total_boosters: number;
  custom_tokens: Record<string, number>;
  user_deck_count: number;
  precon_deck_count: number;
}

export interface GrantedCard {
  grp_id: number;
  set_code: string;
  card_added: boolean;
  gems_compensation: number;
  vault_progress: number;
}

export interface CosmeticArtStyle {
  grp_id: number;
  art_id: number;
}

export interface PlayerCosmetics {
  art_styles: CosmeticArtStyle[];
  avatars: string[];
  sleeves: string[];
  pets: string[];
  emotes: string[];
  titles: string[];
  other: Record<string, unknown[]>;
}

export interface CosmeticsChange {
  art_styles: CosmeticArtStyle[];
  avatars: string[];
  sleeves: string[];
  pets: string[];
  emotes: string[];
  titles: string[];
  other: Record<string, unknown[]>;
}

export interface MasteryState {
  raw: unknown;
  node_count: number;
  milestone_count: number;
  milestones_completed: number;
  current_level: number;
  max_level: number;
  current_xp: number;
  has_premium: boolean;
  nodes_completed: number;
}

export interface CosmeticsUpdatedPayload {
  cosmetics: PlayerCosmetics;
}

export interface MasteryUpdatedPayload {
  state: MasteryState;
}

export interface InventoryChange {
  source: string;
  granted_cards: GrantedCard[];
  gold_delta: number;
  gems_delta: number;
  wc_common_delta: number;
  wc_uncommon_delta: number;
  wc_rare_delta: number;
  wc_mythic_delta: number;
  wc_track_position_delta: number;
  vault_progress_delta: number;
  xp_delta: number;
  boosters: BoosterStack[];
  cosmetics: CosmeticsChange;
  custom_tokens_delta: Record<string, number>;
  /** Event name from courses cache (present for event-related sources). */
  event_name?: string;
}

export interface LogStatus {
  running: boolean;
  log_path: string | null;
  has_inventory: boolean;
}

export interface WatcherStartedPayload {
  log_path: string;
}

export interface WatcherStoppedPayload {
  reason: string;
}

export interface SessionStartedPayload {
  inventory: PlayerInventory;
  is_refresh: boolean;
}

export interface MatchStartingPayload {
  game_mode: string;
}

export interface MatchResultPayload {
  result: {
    match_id: string;
    event_id: string;
    opponent_name: string;
    result: "Victory" | "Defeat" | "Draw" | "Unknown";
    completed_reason: string;
    game_mode: string | null;
  };
}

export interface AchievementsUpdatedPayload {
  achievements: unknown;
  is_refresh: boolean;
}

export interface InventoryUpdatedPayload {
  changes: InventoryChange[];
}

export interface CardsGrantedPayload {
  cards: GrantedCard[];
  source: string;
}

export interface LogEventPayload {
  event_type: string;
  summary: string;
  raw_size: number;
  details?: Record<string, any>;
}

export interface EventCourse {
  course_id: string;
  internal_event_name: string;
  current_module: string;
  wins: number | null;
  losses: number | null;
  max_wins: number | null;
  max_losses: number | null;
}

export interface EventsUpdatedPayload {
  courses: EventCourse[];
}

// --- Detail shapes for enriched log events ---

export interface QuestDetail {
  title: string;
  progress: number;
  goal: number;
}

export interface QuestUpdateDetails {
  quests: QuestDetail[];
  can_swap: boolean;
}

export interface RankDetail {
  class: string;
  tier: number;
  step: number;
  won: number;
  lost?: number;
}

export interface RankInfoDetails {
  constructed: RankDetail | null;
  limited: RankDetail | null;
}

export interface DeckCard {
  grp_id: number;
  quantity: number;
}

export interface DeckUpdateDetails {
  name: string;
  format?: string;
  mainboard?: DeckCard[];
  sideboard?: DeckCard[];
}

export interface EventProgressDetails {
  event_name: string;
  module: string;
  wins: number;
  losses: number;
  max_wins: number;
  max_losses: number;
}

export function startLogWatcher(): Promise<string> {
  return invoke<string>("start_log_watcher");
}

export function stopLogWatcher(): Promise<void> {
  return invoke<void>("stop_log_watcher");
}

export function getInventory(): Promise<PlayerInventory | null> {
  return invoke<PlayerInventory | null>("get_inventory");
}

export function getLogStatus(): Promise<LogStatus> {
  return invoke<LogStatus>("get_log_status");
}

export function getEventCourses(): Promise<EventCourse[]> {
  return invoke<EventCourse[]>("get_event_courses");
}

// --- Collection commands ---

export function getAllCards(): Promise<Record<number, CardInfo>> {
  return invoke<Record<number, CardInfo>>("get_all_cards");
}

export function getCollection(): Promise<Record<number, number> | null> {
  return invoke<Record<number, number> | null>("get_collection");
}

export function startCollectionWatch(): Promise<void> {
  return invoke<void>("start_collection_watch");
}

export function stopCollectionWatch(): Promise<void> {
  return invoke<void>("stop_collection_watch");
}

// --- Memory inventory types and commands ---

export interface MemoryInventoryScalars {
  wc_common: number;
  wc_uncommon: number;
  wc_rare: number;
  wc_mythic: number;
  gold: number;
  gems: number;
  wc_track_position: number;
  vault_progress: number;
}

export interface InventoryScannedPayload {
  scalars: MemoryInventoryScalars;
  candidates_found: number;
  address_hex: string;
}

export interface InventoryChangedPayload {
  previous: MemoryInventoryScalars;
  current: MemoryInventoryScalars;
}

export function getMergedInventory(): Promise<PlayerInventory | null> {
  return invoke<PlayerInventory | null>("get_merged_inventory");
}

export function scanInventory(): Promise<MemoryInventoryScalars> {
  return invoke<MemoryInventoryScalars>("scan_inventory");
}

// --- Cosmetics + Mastery commands ---

export function getCosmetics(): Promise<PlayerCosmetics> {
  return invoke<PlayerCosmetics>("get_cosmetics");
}

export function getMastery(): Promise<MasteryState | null> {
  return invoke<MasteryState | null>("get_mastery");
}

// --- Service registry types and commands ---

export interface ServiceStatus {
  id: string;
  name: string;
  state: string;
  detail: string;
  path: string | null;
}

export interface DiagnosticsExport {
  app_version: string;
  services: ServiceStatus[];
  timestamp_epoch_s: number;
}

export function getServiceStatuses(): Promise<ServiceStatus[]> {
  return invoke<ServiceStatus[]>("get_service_statuses");
}

export function exportDiagnostics(): Promise<DiagnosticsExport> {
  return invoke<DiagnosticsExport>("export_diagnostics");
}

// --- FeedDb types and commands ---

export interface FeedEntryInput {
  timestamp: number;
  session_id: string;
  category: string;
  kind: string;
  icon: string | null;
  title: string;
  details: string | null;
  deltas_json: string | null;
  thumbnails_json: string | null;
}

export interface FeedEntryRow {
  id: number;
  timestamp: number;
  session_id: string;
  category: string;
  kind: string;
  icon: string | null;
  title: string;
  details: string | null;
  deltas_json: string | null;
  thumbnails_json: string | null;
}

export interface SessionSummaryRow {
  session_id: string;
  entry_count: number;
  first_timestamp: number;
  last_timestamp: number;
}

export function insertFeedEntry(entry: FeedEntryInput): Promise<number> {
  return invoke<number>("insert_feed_entry", { entry });
}

export function getFeedEntries(limit: number, beforeId: number | null): Promise<FeedEntryRow[]> {
  return invoke<FeedEntryRow[]>("get_feed_entries", { limit, beforeId });
}

export function getFeedSessions(): Promise<SessionSummaryRow[]> {
  return invoke<SessionSummaryRow[]>("get_feed_sessions");
}

// --- HistoryDb types and commands ---

export interface SnapshotSavedPayload {
  snapshot_type: string;
  snapshot_id: number;
  trigger: string;
}

export interface EconomySnapshotRow {
  id: number;
  timestamp: number;
  trigger: string;
  gold: number;
  gems: number;
  vault_progress: number;
  wc_common: number;
  wc_uncommon: number;
  wc_rare: number;
  wc_mythic: number;
  wc_track_position: number;
  draft_tokens: number;
  sealed_tokens: number;
  total_boosters: number;
  boosters_json: string;
  tokens_json: string;
}

export interface CollectionSnapshotSummary {
  id: number;
  timestamp: number;
  trigger: string;
  unique_cards: number;
  total_copies: number;
  scan_score: number | null;
}

export interface CardGrantRow {
  id: number;
  timestamp: number;
  grp_id: number;
  set_code: string;
  source: string;
  card_added: boolean;
  gems_compensation: number;
  vault_progress: number;
}

export interface SnapshotSummary {
  economy_id: number | null;
  collection_id: number | null;
  cosmetics_id: number | null;
  mastery_id: number | null;
}

export function getEconomyHistory(
  fromTs?: number,
  toTs?: number,
  limit?: number,
): Promise<EconomySnapshotRow[]> {
  return invoke<EconomySnapshotRow[]>("get_economy_history", { fromTs, toTs, limit });
}

export function getCollectionSnapshots(
  fromTs?: number,
  toTs?: number,
  limit?: number,
): Promise<CollectionSnapshotSummary[]> {
  return invoke<CollectionSnapshotSummary[]>("get_collection_snapshots", { fromTs, toTs, limit });
}

export function getCollectionSnapshotDetail(
  id: number,
): Promise<Record<number, number> | null> {
  return invoke<Record<number, number> | null>("get_collection_snapshot_detail", { id });
}

export function getCardGrants(
  fromTs?: number,
  toTs?: number,
  limit?: number,
  grpIdFilter?: number,
): Promise<CardGrantRow[]> {
  return invoke<CardGrantRow[]>("get_card_grants", { fromTs, toTs, limit, grpIdFilter });
}

export function takeSnapshot(): Promise<SnapshotSummary> {
  return invoke<SnapshotSummary>("take_snapshot");
}

// --- SchemaObserver types and commands ---

export type DriftType = "FieldDisappeared" | "FieldAppeared" | "TypeChanged";

export interface DriftEntry {
  event_name: string;
  field_name: string;
  drift_type: DriftType;
  detail: string;
}

export function getLogSchemaDrift(): Promise<DriftEntry[]> {
  return invoke<DriftEntry[]>("get_log_schema_drift");
}

// --- ImageCacheService types and commands ---

export interface ImageCacheProgressPayload {
  completed: number;
  total: number;
  new_files: string[];
}

export interface ImageCacheCompletePayload {
  total_downloaded: number;
  total_skipped: number;
  total_failed: number;
}

export interface ImageCacheStatus {
  cache_dir: string;
  cached_files: string[];
}

export interface ImageSyncEntry {
  grp_id: number;
  url: string;
}

export function getImageCacheStatus(): Promise<ImageCacheStatus> {
  return invoke<ImageCacheStatus>("get_image_cache_status");
}

export function syncImageCache(entries: ImageSyncEntry[]): Promise<void> {
  return invoke<void>("sync_image_cache", { entries });
}
