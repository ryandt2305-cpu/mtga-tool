// Community-data fetch orchestration for the Deck Builder.
//
// All external hosts (EDHREC, Archidekt, Spellbook, 17Lands) are contacted
// through `DeckService::fetch_cached`, which handles per-host spacing,
// caching, degrade-after-failures, and daily caps. This module composes
// them into the shape `assemble_community` wants.

use std::collections::{HashMap, HashSet};

use tauri::AppHandle;

use crate::models::CardInfo;
use crate::services::deck::engine::pool::effective_identity;
use crate::services::deck::engine::types::{
    mask_from_letters, BuildRequest, CommunityData, Format, Seed,
};
use crate::services::deck::sources::{
    archidekt_deck_url, archidekt_list_url, assemble_community, build_staples, edhrec_url,
    parse_archidekt_deck, parse_archidekt_list, parse_edhrec, parse_seventeenlands,
    parse_spellbook, seventeenlands_url, spellbook_url, staples_cache_key, EdhrecCard,
    SeedDeckDetail, SeedDeckSummary, SourceKind, StaplesAggregate, STAPLES_DECK_COUNT,
    STAPLES_MIN_DECKS, STAPLES_TTL_SECS,
};
use crate::services::deck::DeckService;
use crate::services::diagnostics::{emit_info, DECK_013};
use crate::services::history_db::epoch_seconds;

/// Max spellbook pages walked per cold fetch. Each page is 100 combos and
/// each request is serialised behind the spellbook host mutex.
pub const SPELLBOOK_MAX_PAGES: u32 = 10;

/// Upper bound on uncached 17Lands sets we'll fetch in a single build.
/// Cached sets (fresh or stale) are always considered — this cap only
/// throttles cold fetches.
pub const SEVENTEENLANDS_MAX_NEW_PER_BUILD: usize = 8;

pub async fn fetch_edhrec(
    app: &AppHandle,
    svc: &DeckService,
    commander_name: &str,
) -> Vec<EdhrecCard> {
    let url = edhrec_url(commander_name);
    let key = format!("edhrec:{}", commander_name.to_lowercase());
    match svc.fetch_cached(app, SourceKind::Edhrec, &key, &url).await {
        Ok(body) => parse_edhrec(&body).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

pub async fn fetch_spellbook_all(
    app: &AppHandle,
    svc: &DeckService,
    format: Format,
) -> Vec<Vec<String>> {
    let mut all = Vec::new();
    for page in 1..=SPELLBOOK_MAX_PAGES {
        let url = spellbook_url(format, page);
        let key = format!("spellbook:{}:{}", format.as_str(), page);
        let body = match svc
            .fetch_cached(app, SourceKind::Spellbook, &key, &url)
            .await
        {
            Ok(b) => b,
            Err(_) => break,
        };
        let (mut combos, has_next) = match parse_spellbook(&body) {
            Ok(v) => v,
            Err(_) => break,
        };
        all.append(&mut combos);
        if !has_next {
            break;
        }
    }
    all
}

pub async fn fetch_seed_detail(
    app: &AppHandle,
    svc: &DeckService,
    deck_id: i64,
) -> Option<SeedDeckDetail> {
    let url = archidekt_deck_url(deck_id);
    let key = format!("archidekt:deck:{}", deck_id);
    let body = svc
        .fetch_cached(app, SourceKind::ArchidektDeck, &key, &url)
        .await
        .ok()?;
    parse_archidekt_deck(&body).ok()
}

pub async fn fetch_seventeenlands_gih(
    app: &AppHandle,
    svc: &DeckService,
    cards: &HashMap<i32, CardInfo>,
    set_codes: &[String],
) -> HashMap<String, f32> {
    let mut out: HashMap<String, f32> = HashMap::new();
    for set in set_codes {
        let url = seventeenlands_url(set);
        let key = format!("17lands:{}", set);
        let body = match svc
            .fetch_cached(app, SourceKind::SeventeenLands, &key, &url)
            .await
        {
            Ok(b) => b,
            Err(_) => continue,
        };
        let rows = match parse_seventeenlands(&body) {
            Ok(v) => v,
            Err(_) => continue,
        };
        for (mtga_id, wr) in rows {
            if let Some(ci) = cards.get(&mtga_id) {
                let name = ci
                    .name
                    .split(" // ")
                    .next()
                    .unwrap_or(&ci.name)
                    .to_lowercase();
                out.entry(name).and_modify(|v| *v = v.max(wr)).or_insert(wr);
            }
        }
    }
    out
}

/// Priority sets first (commander / build-around), then every set already in
/// the 17Lands cache (fresh or stale — zero network), then uncached sets by
/// owned-card count desc, at most `cap` of them. Deterministic (ties by code).
pub fn select_17lands_sets(
    collection: &HashMap<i32, i32>,
    cards: &HashMap<i32, CardInfo>,
    priority: &[String],
    cached: &HashSet<String>,
    cap: usize,
) -> Vec<String> {
    let mut owned_by_set: HashMap<String, i32> = HashMap::new();
    for (grp, qty) in collection {
        if *qty <= 0 {
            continue;
        }
        if let Some(ci) = cards.get(grp) {
            if !ci.set_code.is_empty() {
                *owned_by_set.entry(ci.set_code.clone()).or_insert(0) += *qty;
            }
        }
    }
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for p in priority {
        if seen.insert(p.clone()) {
            out.push(p.clone());
        }
    }
    let mut rest: Vec<(String, i32)> = owned_by_set
        .into_iter()
        .filter(|(s, _)| !seen.contains(s))
        .collect();
    rest.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    for (s, _) in rest.iter().filter(|(s, _)| cached.contains(s)) {
        if seen.insert(s.clone()) {
            out.push(s.clone());
        }
    }
    let mut new_added = 0usize;
    for (s, _) in rest.iter().filter(|(s, _)| !cached.contains(s)) {
        if new_added >= cap {
            break;
        }
        if seen.insert(s.clone()) {
            out.push(s.clone());
            new_added += 1;
        }
    }
    out
}

/// Return the subset of `candidates` that already have any (fresh or stale)
/// entry in the 17Lands cache. Zero network — a `cache_get` lookup only.
fn cached_17lands_sets(
    svc: &DeckService,
    candidates: impl Iterator<Item = String>,
) -> HashSet<String> {
    let store = svc.store.lock().unwrap();
    let now = epoch_seconds();
    candidates
        .filter(|s| {
            matches!(
                store.cache_get("17lands", &format!("17lands:{}", s), now),
                Ok(Some(_))
            )
        })
        .collect()
}

/// Staples aggregate for a format: one synthetic cache row (7 d). Rebuilds
/// from top-N Archidekt decks (list page 1 + per-deck details, each cached)
/// only when the row is missing/stale. Returns None when < STAPLES_MIN_DECKS
/// details could be fetched (engine then simply has no staple signal).
pub async fn ensure_staples(
    app: &AppHandle,
    svc: &DeckService,
    format: Format,
) -> Option<StaplesAggregate> {
    let key = staples_cache_key(format);
    let now = epoch_seconds();
    let stale_body: Option<String> = {
        let store = svc.store.lock().unwrap();
        match store.cache_get("archidekt", &key, now) {
            Ok(Some(hit)) if hit.fresh => return serde_json::from_str(&hit.body).ok(),
            Ok(Some(hit)) => Some(hit.body),
            _ => None,
        }
    };
    let list_url = archidekt_list_url(format, 1);
    let list_key = format!("archidekt:list:{}:1", format.as_str());
    let body = match svc
        .fetch_cached(app, SourceKind::ArchidektList, &list_key, &list_url)
        .await
    {
        Ok(b) => b,
        Err(_) => return stale_body.and_then(|b| serde_json::from_str(&b).ok()),
    };
    let (items, _) = match parse_archidekt_list(&body) {
        Ok(v) => v,
        Err(_) => return stale_body.and_then(|b| serde_json::from_str(&b).ok()),
    };
    let want = format.deck_size() + 1; // 100 / 60 incl. commander
    let picked: Vec<SeedDeckSummary> = items
        .into_iter()
        .filter(|s| s.size == want)
        .take(STAPLES_DECK_COUNT)
        .collect();
    let mut pairs: Vec<(SeedDeckSummary, SeedDeckDetail)> = Vec::new();
    for s in picked {
        if let Some(d) = fetch_seed_detail(app, svc, s.id).await {
            pairs.push((s, d));
        }
    }
    if pairs.len() < STAPLES_MIN_DECKS {
        return stale_body.and_then(|b| serde_json::from_str(&b).ok());
    }
    let refs: Vec<(&SeedDeckSummary, &SeedDeckDetail)> =
        pairs.iter().map(|(s, d)| (s, d)).collect();
    let agg = build_staples(now, &refs);
    if let Ok(json) = serde_json::to_string(&agg) {
        let store = svc.store.lock().unwrap();
        let _ = store.cache_put("archidekt", &key, &json, STAPLES_TTL_SECS, now);
    }
    let distinct: HashSet<&String> = agg.decks.iter().flat_map(|d| d.cards.iter()).collect();
    emit_info(
        app,
        &DECK_013,
        &format!(
            "staples aggregate rebuilt: {} decks, {} cards ({})",
            agg.decks.len(),
            distinct.len(),
            format.as_str()
        ),
    );
    Some(agg)
}

fn name_of_grp(cards: &HashMap<i32, CardInfo>, grp: i32) -> Option<String> {
    cards.get(&grp).map(|c| c.name.clone())
}

pub async fn community_for(
    app: &AppHandle,
    svc: &DeckService,
    req: &BuildRequest,
    cards: &HashMap<i32, CardInfo>,
    collection: &HashMap<i32, i32>,
) -> CommunityData {
    // EDHREC keyed on the commander's name.
    let commander_name = req
        .commander_grp
        .and_then(|grp| name_of_grp(cards, grp))
        .unwrap_or_default();

    // Derive engine identity (commander color identity ∩ user color subset).
    let identity = req
        .commander_grp
        .and_then(|g| cards.get(&g))
        .map(|ci| mask_from_letters(&ci.color_identity))
        .map(|m| effective_identity(m, req.color_subset.as_deref()).unwrap_or(m))
        .unwrap_or(0);

    // Five sources run concurrently. Each host has its own mutex + spacing
    // inside fetch_cached; cross-host requests do not contend.
    let edhrec_fut = async {
        if commander_name.is_empty() {
            Vec::new()
        } else {
            fetch_edhrec(app, svc, &commander_name).await
        }
    };

    let seed_fut = async {
        match &req.seed {
            Some(Seed::Archidekt { deck_id }) => fetch_seed_detail(app, svc, *deck_id).await,
            _ => None,
        }
    };

    let combos_fut = fetch_spellbook_all(app, svc, req.format);

    let gih_fut = async {
        if req.use_17lands {
            let mut priority: Vec<String> = Vec::new();
            let mut seen: HashSet<String> = HashSet::new();
            if let Some(grp) = req.commander_grp {
                if let Some(ci) = cards.get(&grp) {
                    if !ci.set_code.is_empty() && seen.insert(ci.set_code.clone()) {
                        priority.push(ci.set_code.clone());
                    }
                }
            }
            for ba in &req.build_around {
                if let Some(ci) = cards.get(&ba.grp_id) {
                    if !ci.set_code.is_empty() && seen.insert(ci.set_code.clone()) {
                        priority.push(ci.set_code.clone());
                    }
                }
            }
            let all_owned_sets: HashSet<String> = collection
                .keys()
                .filter_map(|g| cards.get(g))
                .map(|c| c.set_code.clone())
                .filter(|s| !s.is_empty())
                .collect();
            let cached = cached_17lands_sets(svc, all_owned_sets.into_iter());
            let sets = select_17lands_sets(
                collection,
                cards,
                &priority,
                &cached,
                SEVENTEENLANDS_MAX_NEW_PER_BUILD,
            );
            fetch_seventeenlands_gih(app, svc, cards, &sets).await
        } else {
            HashMap::new()
        }
    };

    let staples_fut = ensure_staples(app, svc, req.format);

    let t_e = std::time::Instant::now();
    let (edhrec, seed, combos, gih, staples) =
        tokio::join!(edhrec_fut, seed_fut, combos_fut, gih_fut, staples_fut);
    eprintln!(
        "[community_for] parallel fetch: {}ms — edhrec={} seed={} combos={} gih={} staples={}",
        t_e.elapsed().as_millis(),
        edhrec.len(),
        if seed.is_some() { 1 } else { 0 },
        combos.len(),
        gih.len(),
        if staples.is_some() { 1 } else { 0 },
    );

    assemble_community(
        if edhrec.is_empty() {
            None
        } else {
            Some(edhrec.as_slice())
        },
        seed.as_ref(),
        &combos,
        &gih,
        staples.as_ref(),
        identity,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Rarity;

    fn card(set: &str) -> CardInfo {
        CardInfo {
            grp_id: 0,
            name: String::new(),
            set_code: set.into(),
            rarity: Rarity::Common,
            collector_number: String::new(),
            cmc: 0,
            power: None,
            toughness: None,
            deck_limit: 4,
            color_identity: Vec::new(),
            colors: Vec::new(),
            types: Vec::new(),
            subtypes: Vec::new(),
            supertypes: Vec::new(),
            is_rebalanced: false,
            rebalanced_grp_id: None,
            is_primary_card: true,
        }
    }

    #[test]
    fn select_17lands_sets_orders_by_owned_count_then_caps_uncached() {
        let mut cards = HashMap::new();
        cards.insert(1, card("BLB"));
        cards.insert(2, card("BLB"));
        cards.insert(3, card("OTJ"));
        cards.insert(4, card("MKM"));
        let mut coll = HashMap::new();
        coll.insert(1, 4);
        coll.insert(2, 4);
        coll.insert(3, 1);
        coll.insert(4, 2);
        let cached: HashSet<String> = ["MKM".to_string()].into_iter().collect();
        // priority first, then cached sets always, then uncached by owned count desc, capped
        let out = select_17lands_sets(&coll, &cards, &["OTJ".into()], &cached, 1);
        assert_eq!(
            out,
            vec!["OTJ".to_string(), "MKM".to_string(), "BLB".to_string()]
        );
        let out2 = select_17lands_sets(&coll, &cards, &[], &cached, 0);
        assert_eq!(out2, vec!["MKM".to_string()]);
    }
}
