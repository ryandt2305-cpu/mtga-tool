// DeckService — the Deck Builder's runtime service.
//
// Owns:
// - `DeckStore` (deckbuilder.db) for parsed Scryfall data, community cache, saved decks.
// - A reqwest client used both for Scryfall bulk fetches and on-demand community
//   sources (EDHREC / Archidekt / Spellbook / 17Lands).
// - Per-host mutexes + last-hit timestamps to serialise requests and honour the
//   250 ms host spacing.
// - Per-host failure counters (for the 3-strikes-degrade rule) and per-session
//   "already warned" sets so DECK-007 / DECK-008 emit once per host per session.
// - `ingest_running` flag to prevent overlapping Scryfall bulk ingests.
//
// Async surface:
// - `ensure_bulk(app, cards, force)` — Scryfall bulk metadata → download → parse →
//   `store.replace_bulk` → grp↔oracle join → role precompute. Non-blocking; the
//   caller `spawn`s it after Phase A.
// - `fetch_cached(app, kind, key, url)` — community fetches through
//   [`decide_fetch`]; serves fresh cache, else honours host spacing + daily cap +
//   degrade rules, returning stale-on-error where possible.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_http::reqwest;

use crate::events;
use crate::models::CardInfo;
use crate::services::diagnostics::{
    emit_info, emit_warning, DECK_002, DECK_003, DECK_005, DECK_006, DECK_007, DECK_008, DECK_012,
};
use crate::services::history_db::epoch_seconds;

use super::engine::roles::{classify, TagIndex};
use super::engine::types::{CommunityData, EngineInput, WildcardBudget};
use super::ingest::{join_grp_to_oracle, parse_oracle_cards, parse_oracle_tags_jsonl};
use super::sources::{
    decide_fetch, FetchDecision, SourceKind, DEGRADE_AFTER_FAILURES, HOST_SPACING_MS,
};
use super::store::DeckStore;

const HOSTS: [&str; 4] = ["edhrec", "archidekt", "spellbook", "17lands"];
const USER_AGENT: &str = "MTGAHub/0.1 (deck-builder)";
const BULK_METADATA_TTL_SECS: i64 = 86_400;
const HTTP_TIMEOUT_SECS: u64 = 20;

#[derive(Debug, Clone, Serialize)]
pub struct DeckStatus {
    pub db_ok: bool,
    pub oracles: u64,
    pub bulk_updated_at: Option<String>,
    pub last_check_at: Option<i64>,
    pub ingest_running: bool,
    pub degraded_hosts: Vec<String>,
}

pub struct DeckService {
    pub store: Mutex<DeckStore>,
    client: reqwest::Client,
    host_locks: HashMap<&'static str, tokio::sync::Mutex<Option<Instant>>>,
    failures: Mutex<HashMap<&'static str, u32>>,
    capped_warned: Mutex<HashSet<&'static str>>,
    degraded_warned: Mutex<HashSet<&'static str>>,
    pub ingest_running: AtomicBool,
}

/// RAII guard that clears `ingest_running` on drop so every exit path from
/// `ensure_bulk` (including panics) releases the flag.
struct RunningGuard<'a>(&'a AtomicBool);
impl<'a> Drop for RunningGuard<'a> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl DeckService {
    pub fn new(app_data_dir: &Path) -> Result<Self, String> {
        let store = DeckStore::open(app_data_dir.join("deckbuilder.db"))?;
        let now = epoch_seconds();
        let _ = store.prune_cache(now);
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
            .build()
            .map_err(|e| format!("DECK-001: HTTP client build failed: {}", e))?;
        let mut host_locks = HashMap::new();
        for h in HOSTS.iter() {
            host_locks.insert(*h, tokio::sync::Mutex::new(None));
        }
        Ok(Self {
            store: Mutex::new(store),
            client,
            host_locks,
            failures: Mutex::new(HashMap::new()),
            capped_warned: Mutex::new(HashSet::new()),
            degraded_warned: Mutex::new(HashSet::new()),
            ingest_running: AtomicBool::new(false),
        })
    }

    pub fn status(&self) -> DeckStatus {
        let (oracles, bulk_updated_at, last_check_at) = {
            let store = match self.store.lock() {
                Ok(s) => s,
                Err(_) => {
                    return DeckStatus {
                        db_ok: false,
                        oracles: 0,
                        bulk_updated_at: None,
                        last_check_at: None,
                        ingest_running: self.ingest_running.load(Ordering::SeqCst),
                        degraded_hosts: vec![],
                    }
                }
            };
            let oracles = store.oracle_count().unwrap_or(0);
            let bulk = store.get_meta("oracle_cards_updated_at").ok().flatten();
            let last = store
                .get_meta("last_check_at")
                .ok()
                .flatten()
                .and_then(|s| s.parse::<i64>().ok());
            (oracles, bulk, last)
        };
        let degraded = {
            let f = self.failures.lock().unwrap();
            f.iter()
                .filter(|(_, n)| **n >= DEGRADE_AFTER_FAILURES)
                .map(|(h, _)| (*h).to_string())
                .collect()
        };
        DeckStatus {
            db_ok: true,
            oracles,
            bulk_updated_at,
            last_check_at,
            ingest_running: self.ingest_running.load(Ordering::SeqCst),
            degraded_hosts: degraded,
        }
    }

    /// Refresh the Scryfall bulk data (oracle cards + oracle tags) and precompute
    /// grp↔oracle joins and roles. Returns `Ok(true)` if data changed on disk,
    /// `Ok(false)` if the cached metadata is still current (or an ingest was
    /// already in flight).
    pub async fn ensure_bulk(
        &self,
        app: &AppHandle,
        cards: HashMap<i32, CardInfo>,
        force: bool,
    ) -> Result<bool, String> {
        if self
            .ingest_running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Ok(false);
        }
        let _guard = RunningGuard(&self.ingest_running);

        let now = epoch_seconds();
        let (last_check, stored_cards_at, stored_tags_at, oracles_present, grp_present) = {
            let store = self.store.lock().unwrap();
            (
                store
                    .get_meta("last_check_at")
                    .ok()
                    .flatten()
                    .and_then(|s| s.parse::<i64>().ok()),
                store.get_meta("oracle_cards_updated_at").ok().flatten(),
                store.get_meta("oracle_tags_updated_at").ok().flatten(),
                store.oracle_count().unwrap_or(0) > 0,
                store.load_grp_oracle().map(|v| !v.is_empty()).unwrap_or(false),
            )
        };
        let skip_metadata = !force
            && last_check
                .map(|t| now - t < BULK_METADATA_TTL_SECS)
                .unwrap_or(false);

        // Step 2: metadata skip path — still run the join if we have oracles but no grp map yet.
        if skip_metadata && oracles_present {
            if !grp_present {
                self.rejoin_and_emit(app, &cards, /*changed=*/ false, stored_cards_at.clone())?;
            } else {
                let _ = app.emit(
                    events::deck::INGEST_COMPLETE,
                    &events::IngestCompletePayload {
                        changed: false,
                        oracles: self.store.lock().unwrap().oracle_count().unwrap_or(0),
                        unresolved_grp: 0,
                    },
                );
            }
            return Ok(false);
        }

        // Step 3: fetch bulk-data metadata
        let (cards_url, cards_updated_at, tags_url, tags_updated_at) =
            match fetch_bulk_metadata(&self.client).await {
                Ok(v) => v,
                Err(e) => {
                    emit_warning(app, &DECK_002, &e);
                    if oracles_present {
                        return Ok(false);
                    }
                    return Err(e);
                }
            };
        {
            let store = self.store.lock().unwrap();
            let _ = store.set_meta("last_check_at", &now.to_string());
        }

        // Step 4: metadata unchanged → skip download
        let unchanged = stored_cards_at.as_deref() == Some(cards_updated_at.as_str())
            && stored_tags_at.as_deref() == Some(tags_updated_at.as_str())
            && oracles_present;
        if unchanged {
            if !grp_present {
                self.rejoin_and_emit(app, &cards, false, Some(cards_updated_at))?;
            } else {
                let store = self.store.lock().unwrap();
                let _ = app.emit(
                    events::deck::INGEST_COMPLETE,
                    &events::IngestCompletePayload {
                        changed: false,
                        oracles: store.oracle_count().unwrap_or(0),
                        unresolved_grp: 0,
                    },
                );
            }
            return Ok(false);
        }

        // Step 5: download + parse + replace
        let _ = app.emit(
            events::deck::INGEST_PROGRESS,
            &events::IngestProgressPayload {
                stage: "download_cards".into(),
                done: 0,
                total: 0,
            },
        );
        let cards_bytes = match http_bytes(&self.client, &cards_url).await {
            Ok(b) => b,
            Err(e) => {
                emit_warning(app, &DECK_003, &e);
                if oracles_present {
                    return Ok(false);
                }
                return Err(e);
            }
        };
        let _ = app.emit(
            events::deck::INGEST_PROGRESS,
            &events::IngestProgressPayload {
                stage: "download_tags".into(),
                done: 0,
                total: 0,
            },
        );
        let tags_bytes = match http_bytes(&self.client, &tags_url).await {
            Ok(b) => b,
            Err(e) => {
                emit_warning(app, &DECK_003, &e);
                if oracles_present {
                    return Ok(false);
                }
                return Err(e);
            }
        };

        // Parse off the runtime thread — bulk oracle JSON is ~30 MB.
        let parsed_cards = tauri::async_runtime::spawn_blocking(move || {
            parse_oracle_cards(&cards_bytes)
        })
        .await
        .map_err(|e| format!("DECK-004: join parse task failed: {}", e))?
        .map_err(|e| {
            emit_warning(app, &DECK_012, &e);
            e
        })?;
        let parsed_tags = tauri::async_runtime::spawn_blocking(move || {
            parse_oracle_tags_jsonl(&tags_bytes)
        })
        .await
        .map_err(|e| format!("DECK-004: join tag parse task failed: {}", e))?
        .map_err(|e| {
            emit_warning(app, &DECK_012, &e);
            e
        })?;
        if parsed_tags.anomalies > 0 {
            emit_warning(
                app,
                &DECK_006,
                &format!(
                    "{} tag hierarchy anomalies (missing parent / cycle)",
                    parsed_tags.anomalies
                ),
            );
        }

        {
            let mut store = self.store.lock().unwrap();
            store
                .replace_bulk(
                    &parsed_cards.oracles,
                    &parsed_cards.arena,
                    &parsed_tags.tags,
                    &parsed_tags.taggings,
                )
                .map_err(|e| {
                    emit_warning(app, &DECK_012, &e);
                    e
                })?;
            let _ = store.set_meta("oracle_cards_updated_at", &cards_updated_at);
            let _ = store.set_meta("oracle_tags_updated_at", &tags_updated_at);
        }

        // Step 6: join grp↔oracle + role precompute + emit COMPLETE(changed=true)
        self.rejoin_and_emit(app, &cards, true, Some(cards_updated_at))?;
        Ok(true)
    }

    fn rejoin_and_emit(
        &self,
        app: &AppHandle,
        cards: &HashMap<i32, CardInfo>,
        changed: bool,
        _bulk_updated_at: Option<String>,
    ) -> Result<(), String> {
        let (join, tag_rows, oracle_rows, taggings) = {
            let store = self.store.lock().unwrap();
            let arena_map = store.arena_id_map()?;
            let name_map = store.name_map()?;
            let join = join_grp_to_oracle(cards, &arena_map, &name_map);
            (
                join,
                store.load_tag_rows()?,
                store.load_oracle_rows()?,
                store.load_taggings()?,
            )
        };
        {
            let mut store = self.store.lock().unwrap();
            store.replace_grp_oracle(&join.rows)?;
        }
        emit_info(
            app,
            &DECK_005,
            &format!("grp↔oracle unresolved: {}", join.unresolved.len()),
        );

        let tag_index = TagIndex::new(&tag_rows);
        let mut tags_by_oracle: HashMap<String, Vec<String>> = HashMap::new();
        for (oracle_id, tag_id) in taggings {
            tags_by_oracle.entry(oracle_id).or_default().push(tag_id);
        }
        let mut role_rows: Vec<(String, String)> = Vec::new();
        for o in &oracle_rows {
            let tag_ids = tags_by_oracle.remove(&o.oracle_id).unwrap_or_default();
            let keywords: Vec<String> = serde_json::from_str(&o.keywords).unwrap_or_default();
            let roles = classify(
                &tag_index,
                &tag_ids,
                &o.type_line,
                &o.oracle_text,
                &keywords,
                o.power,
            );
            for r in roles {
                role_rows.push((o.oracle_id.clone(), r.as_str().to_string()));
            }
        }
        {
            let mut store = self.store.lock().unwrap();
            store.replace_roles(&role_rows)?;
        }

        let oracles = self.store.lock().unwrap().oracle_count().unwrap_or(0);
        let _ = app.emit(
            events::deck::INGEST_COMPLETE,
            &events::IngestCompletePayload {
                changed,
                oracles,
                unresolved_grp: join.unresolved.len() as u64,
            },
        );
        Ok(())
    }

    /// On-demand community fetch with cache, host spacing, daily cap, and
    /// degrade-after-N-failures. See [`decide_fetch`] for the pure policy.
    pub async fn fetch_cached(
        &self,
        app: &AppHandle,
        kind: SourceKind,
        key: &str,
        url: &str,
    ) -> Result<String, String> {
        let host = kind.host();
        let host_lock = self
            .host_locks
            .get(host)
            .ok_or_else(|| format!("DECK-007: unknown host {}", host))?;
        let mut last_hit_guard = host_lock.lock().await;

        let now = epoch_seconds();
        let today = day_key(now);
        let (fresh_hit, failures, hits) = {
            let store = self.store.lock().unwrap();
            let hit = store.cache_get(host, key, now)?;
            let f = *self.failures.lock().unwrap().get(host).unwrap_or(&0);
            let h = store.hits_today(host, &today)?;
            (hit, f, h)
        };

        match decide_fetch(fresh_hit, failures, hits) {
            FetchDecision::UseCache(body) => Ok(body),
            FetchDecision::Blocked { stale, code } => {
                self.warn_once(app, code, host);
                stale.ok_or_else(|| format!("{}: {} unavailable", code, host))
            }
            FetchDecision::Fetch { stale } => {
                if let Some(prev) = *last_hit_guard {
                    let elapsed = prev.elapsed();
                    let need = Duration::from_millis(HOST_SPACING_MS);
                    if elapsed < need {
                        tokio::time::sleep(need - elapsed).await;
                    }
                }
                *last_hit_guard = Some(Instant::now());
                let res = self
                    .client
                    .get(url)
                    .header("Accept", "application/json")
                    .send()
                    .await;
                {
                    let store = self.store.lock().unwrap();
                    let _ = store.hit_increment(host, &today);
                }
                match res.and_then(|r| r.error_for_status()) {
                    Ok(resp) => match resp.text().await {
                        Ok(body) => {
                            {
                                let store = self.store.lock().unwrap();
                                let _ = store.cache_put(host, key, &body, kind.ttl_secs(), now);
                            }
                            self.failures.lock().unwrap().insert(host, 0);
                            let _ = app.emit(
                                events::deck::SOURCES_UPDATED,
                                &events::SourcesUpdatedPayload {
                                    host: host.to_string(),
                                    key: key.to_string(),
                                },
                            );
                            Ok(body)
                        }
                        Err(e) => self.on_fetch_error(app, host, stale, format!("body read: {}", e)),
                    },
                    Err(e) => self.on_fetch_error(app, host, stale, format!("{}", e)),
                }
            }
        }
    }

    fn on_fetch_error(
        &self,
        app: &AppHandle,
        host: &'static str,
        stale: Option<String>,
        msg: String,
    ) -> Result<String, String> {
        let n = {
            let mut f = self.failures.lock().unwrap();
            let e = f.entry(host).or_insert(0);
            *e += 1;
            *e
        };
        if n >= DEGRADE_AFTER_FAILURES {
            self.warn_once(app, "DECK-007", host);
        }
        if let Some(body) = stale {
            log::warn!("DECK-007: {} fetch failed ({}) — serving stale cache", host, msg);
            Ok(body)
        } else {
            let full = format!("DECK-007: {} fetch failed: {}", host, msg);
            log::warn!("{}", full);
            Err(full)
        }
    }

    fn warn_once(&self, app: &AppHandle, code: &str, host: &'static str) {
        let set: &Mutex<HashSet<&'static str>> = match code {
            "DECK-007" => &self.degraded_warned,
            "DECK-008" => &self.capped_warned,
            _ => return,
        };
        let mut s = set.lock().unwrap();
        if s.insert(host) {
            let msg = format!("{}: {}", code, host);
            match code {
                "DECK-007" => emit_warning(app, &DECK_007, &msg),
                "DECK-008" => emit_warning(app, &DECK_008, &msg),
                _ => {}
            }
        }
    }

    /// Compose an `EngineInput`: load oracles from the store, then attach owned
    /// wildcards and community signals for the current build request.
    pub fn build_input(
        &self,
        cards: &HashMap<i32, CardInfo>,
        collection: &HashMap<i32, i32>,
        wildcards: WildcardBudget,
        community: CommunityData,
    ) -> Result<EngineInput, String> {
        let store = self.store.lock().unwrap();
        let oracles = store.load_oracles(cards, collection)?;
        Ok(EngineInput {
            oracles,
            wildcards_owned: wildcards,
            community,
        })
    }
}

async fn fetch_bulk_metadata(
    client: &reqwest::Client,
) -> Result<(String, String, String, String), String> {
    let resp = client
        .get("https://api.scryfall.com/bulk-data")
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("bulk-data GET failed: {}", e))?
        .error_for_status()
        .map_err(|e| format!("bulk-data status: {}", e))?;
    let text = resp
        .text()
        .await
        .map_err(|e| format!("bulk-data body: {}", e))?;
    let v: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("bulk-data parse: {}", e))?;
    let data = v
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| "bulk-data: missing 'data' array".to_string())?;
    let mut cards = None;
    let mut tags = None;
    for entry in data {
        let ty = entry.get("type").and_then(|s| s.as_str()).unwrap_or("");
        let updated_at = entry
            .get("updated_at")
            .and_then(|s| s.as_str())
            .unwrap_or("");
        if ty == "oracle_cards" {
            let url = entry
                .get("jsonl_download_uri")
                .and_then(|s| s.as_str())
                .or_else(|| entry.get("download_uri").and_then(|s| s.as_str()))
                .unwrap_or("");
            cards = Some((url.to_string(), updated_at.to_string()));
        } else if ty == "oracle_tags" {
            let url = entry
                .get("jsonl_download_uri")
                .and_then(|s| s.as_str())
                .or_else(|| entry.get("download_uri").and_then(|s| s.as_str()))
                .unwrap_or("");
            tags = Some((url.to_string(), updated_at.to_string()));
        }
    }
    let (cards_url, cards_at) =
        cards.ok_or_else(|| "bulk-data: missing oracle_cards entry".to_string())?;
    let (tags_url, tags_at) =
        tags.ok_or_else(|| "bulk-data: missing oracle_tags entry".to_string())?;
    if cards_url.is_empty() || tags_url.is_empty() {
        return Err("bulk-data: empty download_uri".into());
    }
    Ok((cards_url, cards_at, tags_url, tags_at))
}

async fn http_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("GET {} failed: {}", url, e))?
        .error_for_status()
        .map_err(|e| format!("GET {} status: {}", url, e))?;
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("GET {} body: {}", url, e))?;
    Ok(bytes.to_vec())
}

fn day_key(epoch: i64) -> String {
    // UTC YYYY-MM-DD — 86 400 s per day, matches `hits_today` semantics.
    let days = epoch.div_euclid(86_400);
    let (y, m, d) = ymd_from_days(days);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

fn ymd_from_days(days_since_epoch: i64) -> (i32, u32, u32) {
    // 1970-01-01 = day 0. Chrono-free civil date algorithm (Howard Hinnant).
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = y + if m <= 2 { 1 } else { 0 };
    (y as i32, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_key_matches_known_dates() {
        assert_eq!(day_key(0), "1970-01-01");
        assert_eq!(day_key(86_399), "1970-01-01");
        assert_eq!(day_key(86_400), "1970-01-02");
        // 2000-01-01 UTC = 946_684_800
        assert_eq!(day_key(946_684_800), "2000-01-01");
        assert_eq!(day_key(946_684_800 - 1), "1999-12-31");
        // 2024-02-29 UTC = 1_709_164_800 (leap day)
        assert_eq!(day_key(1_709_164_800), "2024-02-29");
    }
}
