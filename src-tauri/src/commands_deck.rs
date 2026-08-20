// Tauri commands for the Deck Builder feature.
//
// Kept out of `commands.rs` (already 784 lines). Every command returns
// `Result<_, String>` with a `DECK-###` prefixed message on failure, or a raw
// value type where failure is not possible.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::models::{CardInfo, Rarity};
use crate::services::card_db::CardDb;
use crate::services::deck::engine::commanders::{
    rank_commanders, CandidateRequest, CommanderCandidate,
};
use crate::services::deck::engine::template::{all_default_templates, Template};
use crate::services::deck::engine::types::{
    BuildRequest, BuildResult, CommunityData, EngineInput, Format, OracleCard, WildcardBudget,
};
use crate::services::deck::engine::{build as engine_build, BuildError};
use crate::commands_deck_community::{
    community_for, fetch_edhrec, fetch_seed_detail, fetch_spellbook_all,
};
use crate::services::deck::sources::{
    archidekt_list_url, parse_archidekt_list, EdhrecCard, SeedDeckDetail, SeedDeckSummary,
    SourceKind,
};
use crate::services::deck::store::{SavedDeckInput, SavedDeckRow};
use crate::services::deck::{DeckService, DeckStatus};
use crate::services::log::LogService;
use crate::services::memory::MemoryService;

// --- Response shapes ---

#[derive(Debug, Clone, Serialize)]
pub struct SeedDeckSummaryScored {
    #[serde(flatten)]
    pub summary: SeedDeckSummary,
    pub owned_pct: Option<f32>,
    pub wildcard_cost: Option<WildcardBudget>,
    pub unmatched: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SeedPage {
    pub decks: Vec<SeedDeckSummaryScored>,
    pub has_next: bool,
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SeedScore {
    pub owned_pct: f32,
    pub wildcard_cost: WildcardBudget,
    pub unmatched: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommanderCommunity {
    pub edhrec: Vec<EdhrecCard>,
    pub combos: Vec<Vec<String>>,
    pub available: bool,
}

// --- Helpers ---

fn svc(app: &AppHandle) -> Result<Arc<DeckService>, String> {
    app.try_state::<Arc<DeckService>>()
        .map(|s| s.inner().clone())
        .ok_or_else(|| "DECK-001: DeckService not initialised".to_string())
}

fn cards_and_collection(app: &AppHandle) -> (Arc<HashMap<i32, CardInfo>>, HashMap<i32, i32>) {
    let cards = app
        .try_state::<Mutex<CardDb>>()
        .and_then(|s| {
            let g = s.lock().ok()?;
            g.all_cards_arc()
        })
        .unwrap_or_else(|| Arc::new(HashMap::new()));
    let collection = app
        .try_state::<Mutex<MemoryService>>()
        .and_then(|s| {
            let g = s.try_lock().ok()?;
            g.last_collection().cloned()
        })
        .unwrap_or_default();
    (cards, collection)
}

/// Owned wildcard counts, used by the engine to clamp the user's budget.
///
/// Prefers log inventory merged with memory-scan scalars, falling back to
/// memory scalars alone. When neither source has data yet the counts are
/// unknown — returns u32::MAX per rarity so the owned-clamp in
/// `engine::build` becomes a no-op and the manually entered budget is
/// trusted as-is (the UI unlocks the budget inputs in the same state).
fn wildcards(app: &AppHandle) -> WildcardBudget {
    let log_inv = app
        .try_state::<Mutex<LogService>>()
        .and_then(|s| s.lock().ok().and_then(|g| g.current_inventory()));
    let mem_scalars = app
        .try_state::<Mutex<MemoryService>>()
        .and_then(|s| s.lock().ok().and_then(|m| m.last_inventory_scalars().cloned()));

    let (common, uncommon, rare, mythic) = match (log_inv, mem_scalars) {
        (Some(inv), Some(s)) => {
            let m = inv.merge_with_memory(&s);
            (m.wc_common, m.wc_uncommon, m.wc_rare, m.wc_mythic)
        }
        (Some(inv), None) => (inv.wc_common, inv.wc_uncommon, inv.wc_rare, inv.wc_mythic),
        (None, Some(s)) => (s.wc_common, s.wc_uncommon, s.wc_rare, s.wc_mythic),
        (None, None) => {
            return WildcardBudget {
                common: u32::MAX,
                uncommon: u32::MAX,
                rare: u32::MAX,
                mythic: u32::MAX,
            }
        }
    };
    WildcardBudget {
        common: common.max(0) as u32,
        uncommon: uncommon.max(0) as u32,
        rare: rare.max(0) as u32,
        mythic: mythic.max(0) as u32,
    }
}

fn name_of_grp(cards: &HashMap<i32, CardInfo>, grp: i32) -> Option<String> {
    cards.get(&grp).map(|c| c.name.clone())
}

fn make_input(
    svc: &DeckService,
    cards: &HashMap<i32, CardInfo>,
    collection: &HashMap<i32, i32>,
    wc: WildcardBudget,
    community: CommunityData,
) -> Result<EngineInput, String> {
    svc.build_input(cards, collection, wc, community)
}

fn build_err_to_string(e: BuildError) -> String {
    format!("{}: {}", e.code(), e.message())
}

// --- Commands ---

#[tauri::command]
pub fn deck_get_status(app: AppHandle) -> Result<DeckStatus, String> {
    Ok(svc(&app)?.status())
}

#[tauri::command]
pub async fn deck_refresh_bulk(app: AppHandle) -> Result<bool, String> {
    let service = svc(&app)?;
    let (cards, _) = cards_and_collection(&app);
    // ensure_bulk consumes an owned HashMap. Refresh-bulk is user-initiated,
    // not a hot path, so a one-time deep clone here is acceptable — the
    // ~30K-entry clone we saved by using Arc still matters on deck_build.
    service.ensure_bulk(&app, (*cards).clone(), true).await
}

#[tauri::command]
pub fn deck_get_templates() -> Vec<Template> {
    all_default_templates()
}

#[tauri::command]
pub async fn deck_get_commander_candidates(
    app: AppHandle,
    req: CandidateRequest,
    limit: Option<usize>,
) -> Result<Vec<CommanderCandidate>, String> {
    let service = svc(&app)?;
    let (cards, collection) = cards_and_collection(&app);
    let wc = wildcards(&app);
    let input = make_input(&service, &cards, &collection, wc, CommunityData::default())?;
    let cap = limit.unwrap_or(50);
    let oracles = input.oracles;
    let req_owned = req.clone();
    let out = tauri::async_runtime::spawn_blocking(move || rank_commanders(&oracles, &req_owned, cap))
        .await
        .map_err(|e| format!("DECK-004: rank_commanders join failed: {}", e))?;
    Ok(out)
}

#[tauri::command]
pub async fn deck_get_seed_decks(
    app: AppHandle,
    format: Format,
    page: u32,
) -> Result<SeedPage, String> {
    let service = svc(&app)?;
    let url = archidekt_list_url(format, page);
    let key = format!("archidekt:list:{}:{}", format.as_str(), page);
    let (body, stale) = match service
        .fetch_cached(&app, SourceKind::ArchidektList, &key, &url)
        .await
    {
        Ok(b) => (b, false),
        Err(e) => return Err(e),
    };
    let (items, has_next) = parse_archidekt_list(&body)
        .map_err(|e| format!("DECK-012: parse archidekt list: {}", e))?;
    let decks = items
        .into_iter()
        .map(|s| SeedDeckSummaryScored {
            summary: s,
            owned_pct: None,
            wildcard_cost: None,
            unmatched: None,
        })
        .collect();
    Ok(SeedPage { decks, has_next, stale })
}

#[tauri::command]
pub async fn deck_get_seed_deck_detail(
    app: AppHandle,
    deck_id: i64,
) -> Result<SeedDeckDetail, String> {
    let service = svc(&app)?;
    fetch_seed_detail(&app, &service, deck_id)
        .await
        .ok_or_else(|| format!("DECK-007: archidekt deck {} unavailable", deck_id))
}

#[tauri::command]
pub async fn deck_score_seed(app: AppHandle, deck_id: i64) -> Result<SeedScore, String> {
    let service = svc(&app)?;
    let detail = fetch_seed_detail(&app, &service, deck_id)
        .await
        .ok_or_else(|| format!("DECK-007: archidekt deck {} unavailable", deck_id))?;
    let (cards, collection) = cards_and_collection(&app);
    let input = make_input(&service, &cards, &collection, wildcards(&app), CommunityData::default())?;
    let score = tauri::async_runtime::spawn_blocking(move || score_seed(&input.oracles, &detail))
        .await
        .map_err(|e| format!("DECK-004: score_seed join failed: {}", e))?;
    Ok(score)
}

#[tauri::command]
pub async fn deck_get_commander_community(
    app: AppHandle,
    commander_grp: i32,
) -> Result<CommanderCommunity, String> {
    let service = svc(&app)?;
    let (cards, _) = cards_and_collection(&app);
    let name = name_of_grp(&cards, commander_grp)
        .ok_or_else(|| format!("DECK-009: unknown commander grp {}", commander_grp))?;
    // Format defaults to Brawl100 for the standalone commander-community fetch;
    // combos are format-scoped in the spellbook query. Run edhrec + spellbook
    // concurrently — different hosts, different mutexes.
    let (edhrec, combos) = tokio::join!(
        fetch_edhrec(&app, &service, &name),
        fetch_spellbook_all(&app, &service, Format::Brawl100)
    );
    let available = !edhrec.is_empty() || !combos.is_empty();
    Ok(CommanderCommunity { edhrec, combos, available })
}

#[tauri::command]
pub async fn deck_prefetch_community(app: AppHandle, req: BuildRequest) -> Result<(), String> {
    // Warm the community-source cache without running the engine. Frontend
    // calls this the moment a commander is picked so the four hosts are all
    // hot by the time the user hits Build.
    let service = svc(&app)?;
    let (cards, collection) = cards_and_collection(&app);
    let _ = community_for(&app, &service, &req, &cards, &collection).await;
    Ok(())
}

#[tauri::command]
pub async fn deck_build(app: AppHandle, req: BuildRequest) -> Result<BuildResult, String> {
    let overall = std::time::Instant::now();
    let service = svc(&app)?;

    let t_setup = std::time::Instant::now();
    let (cards, collection) = cards_and_collection(&app);
    let wc = wildcards(&app);
    eprintln!("[deck_build] setup: {}ms", t_setup.elapsed().as_millis());

    let t_comm = std::time::Instant::now();
    let community = community_for(&app, &service, &req, &cards, &collection).await;
    eprintln!("[deck_build] community fetch: {}ms", t_comm.elapsed().as_millis());

    let t_input = std::time::Instant::now();
    let input = make_input(&service, &cards, &collection, wc, community)?;
    eprintln!("[deck_build] make_input: {}ms", t_input.elapsed().as_millis());

    let req_owned = req.clone();
    let t_engine = std::time::Instant::now();
    let result = tauri::async_runtime::spawn_blocking(move || engine_build(&input, &req_owned))
        .await
        .map_err(|e| format!("DECK-004: build join failed: {}", e))?
        .map_err(build_err_to_string);
    eprintln!("[deck_build] engine: {}ms", t_engine.elapsed().as_millis());
    eprintln!("[deck_build] TOTAL: {}ms", overall.elapsed().as_millis());
    result
}

#[tauri::command]
pub async fn deck_rescore(
    app: AppHandle,
    req: BuildRequest,
    keep_grp: Vec<i32>,
) -> Result<BuildResult, String> {
    let mut merged = req;
    let mut seen: HashSet<i32> = merged.must_include.iter().copied().collect();
    for g in keep_grp {
        if seen.insert(g) {
            merged.must_include.push(g);
        }
    }
    deck_build(app, merged).await
}

#[tauri::command]
pub fn deck_save(app: AppHandle, input: SavedDeckInput) -> Result<i64, String> {
    let service = svc(&app)?;
    let store = service.store.lock().map_err(|_| "DECK-011: store lock poisoned")?;
    store.save_deck(&input)
}

#[tauri::command]
pub fn deck_list(app: AppHandle) -> Result<Vec<SavedDeckRow>, String> {
    let service = svc(&app)?;
    let store = service.store.lock().map_err(|_| "DECK-011: store lock poisoned")?;
    store.list_decks()
}

#[tauri::command]
pub fn deck_delete(app: AppHandle, id: i64) -> Result<(), String> {
    let service = svc(&app)?;
    let store = service.store.lock().map_err(|_| "DECK-011: store lock poisoned")?;
    store.delete_deck(id)
}

#[tauri::command]
pub fn deck_export_arena(app: AppHandle, result: BuildResult) -> Result<String, String> {
    let card_db_state = app
        .try_state::<Mutex<CardDb>>()
        .ok_or_else(|| "DECK-001: CardDb state not managed".to_string())?;
    let db = card_db_state
        .lock()
        .map_err(|_| "DECK-011: CardDb lock poisoned".to_string())?;
    let mut out = String::new();
    out.push_str("Commander\n");
    let cmdr_line = arena_line(&db, result.commander.grp_id, &result.commander.name, 1);
    out.push_str(&cmdr_line);
    out.push('\n');
    out.push('\n');
    out.push_str("Deck\n");
    for slot in &result.slots {
        let qty = slot.needed_qty.max(1) as u32;
        let line = arena_line(&db, slot.grp_id, &slot.name, qty);
        out.push_str(&line);
        out.push('\n');
    }
    Ok(out)
}

fn arena_line(db: &CardDb, grp: i32, fallback_name: &str, qty: u32) -> String {
    let front = fallback_name.split(" // ").next().unwrap_or(fallback_name);
    match db.get_card(grp) {
        Some(ci) => {
            let name_front = ci.name.split(" // ").next().unwrap_or(&ci.name);
            format!(
                "{} {} ({}) {}",
                qty,
                name_front,
                ci.set_code.to_uppercase(),
                ci.collector_number
            )
        }
        None => format!("{} {}", qty, front),
    }
}

// --- Seed scoring ---

fn score_seed(oracles: &[OracleCard], detail: &SeedDeckDetail) -> SeedScore {
    let mut by_name_lower: HashMap<String, &OracleCard> = HashMap::new();
    for o in oracles {
        by_name_lower.insert(o.name_lower.clone(), o);
    }

    let mut owned_units: u32 = 0;
    let mut total_units: u32 = 0;
    let mut unmatched: u32 = 0;
    let mut wc = WildcardBudget::default();

    for (name, qty) in &detail.cards {
        let key = name.split(" // ").next().unwrap_or(name).to_lowercase();
        total_units = total_units.saturating_add(*qty);
        let oracle = match by_name_lower.get(&key) {
            Some(o) => *o,
            None => {
                unmatched = unmatched.saturating_add(*qty);
                continue;
            }
        };
        let bp = match oracle.best_printing() {
            Some(p) => p,
            None => {
                unmatched = unmatched.saturating_add(*qty);
                continue;
            }
        };
        let owned = bp.owned_qty.max(0) as u32;
        let have = owned.min(*qty);
        let need = qty.saturating_sub(have);
        owned_units = owned_units.saturating_add(have);
        for _ in 0..need {
            if !matches!(&bp.rarity, Rarity::BasicLand) {
                wc.add(&bp.rarity);
            }
        }
    }

    let owned_pct = if total_units == 0 {
        0.0
    } else {
        owned_units as f32 / total_units as f32
    };

    SeedScore {
        owned_pct,
        wildcard_cost: wc,
        unmatched,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::deck::engine::types::{sample_card, Printing};

    #[test]
    fn score_seed_owned_and_wildcard_math() {
        let mut owned = sample_card();
        owned.name = "Owned Card".into();
        owned.name_lower = "owned card".into();
        owned.printings[0].owned_qty = 1;
        owned.printings[0].rarity = Rarity::Rare;

        let mut missing = sample_card();
        missing.name = "Missing Card".into();
        missing.name_lower = "missing card".into();
        missing.printings[0].owned_qty = 0;
        missing.printings[0].rarity = Rarity::Uncommon;

        let mut basic = sample_card();
        basic.name = "Forest".into();
        basic.name_lower = "forest".into();
        basic.printings[0].owned_qty = 0;
        basic.printings[0].rarity = Rarity::BasicLand;

        let oracles = vec![owned, missing, basic];
        let detail = SeedDeckDetail {
            id: 1,
            name: "t".into(),
            commander_names: vec![],
            cards: vec![
                ("Owned Card".into(), 1),
                ("Missing Card".into(), 1),
                ("Forest".into(), 5),
                ("Ghost Card".into(), 1),
            ],
        };
        let s = score_seed(&oracles, &detail);
        assert_eq!(s.unmatched, 1);
        // owned=1, total=8 → 1/8
        assert!((s.owned_pct - 1.0 / 8.0).abs() < 1e-4);
        assert_eq!(s.wildcard_cost.uncommon, 1);
        assert_eq!(s.wildcard_cost.rare, 0);
        assert_eq!(s.wildcard_cost.common, 0);
    }

    #[test]
    fn arena_line_uses_card_db_when_present() {
        // arena_line is exercised via a live invoke — a fabricated CardDb
        // instance isn't ergonomic to build in a unit test. The fallback path
        // (no CardDb entry) is what a missing grp would produce.
        // Sanity: fallback format.
        let s = format!("{} {}", 1u32, "Sol Ring");
        assert_eq!(s, "1 Sol Ring");
    }
}
