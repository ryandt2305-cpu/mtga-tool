// Extracted handler bodies for log event processing.
//
// Called by watcher.rs dispatch functions. Each handler takes the parsed JSON
// data + relevant Arc<Mutex> state and emits typed events via Tauri.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::events;
use crate::models::inventory::{DeckContents, PlayerCosmetics, PlayerInventory};
use crate::models::{EventCourse, MasteryState};
use crate::models::{ActiveMatchState, MatchOutcome, MatchResult};
use crate::services::diagnostics::{self, LOG_005, LOG_006, LOG_009, LOG_010, LOG_012, LOG_018};
use crate::services::history_db::HistoryDb;
use crate::services::log::schema::SchemaObserver;
use crate::services::log::{decks, inventory};

/// Handle a StartHook event — parse inventory, cosmetics, mastery, decks, achievements; store and emit.
pub fn handle_start_hook(
    app: &AppHandle,
    data: &Value,
    inventory_arc: &Arc<Mutex<Option<PlayerInventory>>>,
    non_collectible_arc: &Arc<Mutex<HashSet<i32>>>,
    anchor_arc: &Arc<Mutex<HashSet<i32>>>,
    _courses_arc: &Arc<Mutex<Vec<EventCourse>>>,
    cosmetics_arc: &Arc<Mutex<PlayerCosmetics>>,
    mastery_arc: &Arc<Mutex<Option<MasteryState>>>,
    decks_arc: &Arc<Mutex<HashMap<String, DeckContents>>>,
    achievements_arc: &Arc<Mutex<Option<Value>>>,
) {
    // Schema observation — best-effort, never blocks parsing
    if let Ok(mut observer) = app.state::<Mutex<SchemaObserver>>().try_lock() {
        observer.check_critical("StartHook", data, app);
        observer.observe("StartHook", data, app);
        if let Some(inv_info) = data.get("InventoryInfo") {
            observer.observe("StartHook.InventoryInfo", inv_info, app);
        }
    }

    match inventory::parse_start_hook(data) {
        Ok((inv, non_collectible, anchors)) => {
            log::info!(
                "StartHook: {}g {}gems, {} boosters",
                inv.gold,
                inv.gems,
                inv.total_boosters,
            );

            let is_refresh = inventory_arc.lock().unwrap().is_some();

            let _ = app.emit(
                events::log::SESSION_STARTED,
                &events::SessionStartedPayload {
                    inventory: inv.clone(),
                    is_refresh,
                },
            );

            // Auto-snapshot on non-refresh session start
            if !is_refresh {
                if let Ok(db) = app.state::<Mutex<HistoryDb>>().try_lock() {
                    let _ = db.save_economy_snapshot("session_start", &inv);
                }
            }

            *inventory_arc.lock().unwrap() = Some(inv);
            *non_collectible_arc.lock().unwrap() = non_collectible;
            *anchor_arc.lock().unwrap() = anchors;
        }
        Err(e) => {
            diagnostics::emit_warning(app, &LOG_005, &format!("StartHook parse failed: {}", e));
        }
    }

    // Parse deck card lists from StartHook
    let summaries = data
        .get("DeckSummaries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let deck_map = decks::parse_deck_map(data, &summaries);
    log::info!("Parsed {} user deck(s) from StartHook", deck_map.len());
    *decks_arc.lock().unwrap() = deck_map;

    // Parse cosmetics from StartHook
    if let Some(inv_info) = data.get("InventoryInfo") {
        if let Some(cosmetics_obj) = inv_info.get("Cosmetics") {
            let cosmetics = inventory::parse_cosmetics(cosmetics_obj, Some(app));
            let _ = app.emit(
                events::log::COSMETICS_UPDATED,
                &events::CosmeticsUpdatedPayload {
                    cosmetics: cosmetics.clone(),
                },
            );
            if let Ok(db) = app.state::<Mutex<HistoryDb>>().try_lock() {
                let _ = db.save_cosmetic_snapshot("session_start", &cosmetics);
            }

            *cosmetics_arc.lock().unwrap() = cosmetics;
        } else {
            diagnostics::emit_info(app, &LOG_010, "StartHook: no Cosmetics object in InventoryInfo");
        }
    }

    // Parse mastery from StartHook (UpdatedGraphs)
    match inventory::parse_updated_graphs(data) {
        Some(state) => {
            log::info!(
                "StartHook mastery: {} nodes, {} milestones",
                state.node_count,
                state.milestone_count,
            );
            let _ = app.emit(
                events::log::MASTERY_UPDATED,
                &events::MasteryUpdatedPayload {
                    state: state.clone(),
                },
            );

            if let Ok(db) = app.state::<Mutex<HistoryDb>>().try_lock() {
                let _ = db.save_mastery_snapshot("session_start", &state);
            }

            *mastery_arc.lock().unwrap() = Some(state);
        }
        None => {
            log::debug!("StartHook: no UpdatedGraphs data found");
        }
    }

    // Extract achievements (discovery-first)
    // achievements_arc being Some means we've seen achievements before in this session
    let achiev_is_refresh = achievements_arc.lock().unwrap().is_some();
    handle_achievements(app, data, achievements_arc, achiev_is_refresh);
}

/// Handle an InventoryUpdate event — parse changes, merge cosmetics, emit events.
///
/// Event-related sources (EventPayEntry, EventReward, Draft, Sealed, etc.) are
/// enriched with the event name from the active courses cache.
pub fn handle_inventory_update(
    app: &AppHandle,
    data: &Value,
    cosmetics_arc: &Arc<Mutex<PlayerCosmetics>>,
    courses_arc: &Arc<Mutex<Vec<EventCourse>>>,
) {
    // Schema observation — best-effort, never blocks parsing
    if let Ok(mut observer) = app.state::<Mutex<SchemaObserver>>().try_lock() {
        observer.check_critical("InventoryUpdate", data, app);
        observer.observe("InventoryUpdate", data, app);
        // Observe first change and first granted card as representative shapes
        if let Some(changes) = data
            .get("InventoryInfo")
            .and_then(|v| v.get("Changes"))
            .and_then(|v| v.as_array())
        {
            if let Some(first_change) = changes.first() {
                observer.observe("InventoryChange", first_change, app);
                if let Some(first_card) = first_change
                    .get("GrantedCards")
                    .and_then(|v| v.as_array())
                    .and_then(|a| a.first())
                {
                    observer.observe("GrantedCard", first_card, app);
                }
            }
        }
    }

    match inventory::parse_inventory_changes(data) {
        Ok(mut changes) => {
            // Enrich event-related sources with event name from courses cache
            {
                let event_sources = [
                    "EventPayEntry", "EventReward", "EventEntryReward", "EntryReward",
                    "Draft", "Sealed", "Constructed", "EventRefundEntry",
                ];
                let courses = courses_arc.lock().unwrap();
                if !courses.is_empty() {
                    for change in &mut changes {
                        if event_sources.contains(&change.source.as_str()) {
                            // Use the most recently active course's event name
                            if let Some(course) = courses.first() {
                                change.event_name = Some(course.internal_event_name.clone());
                            }
                        }
                    }
                }
            }

            for change in &changes {
                log::info!(
                    "InventoryChange: source={}, cards={}, gold={:+}, gems={:+}, wc_rare={:+}, tokens={}{}",
                    change.source,
                    change.granted_cards.len(),
                    change.gold_delta,
                    change.gems_delta,
                    change.wc_rare_delta,
                    change.custom_tokens_delta.len(),
                    change.event_name.as_ref().map(|n| format!(", event={}", n)).unwrap_or_default(),
                );
            }

            // Emit cards_granted and persist grant log for each change
            for change in &changes {
                if !change.granted_cards.is_empty() {
                    let _ = app.emit(
                        events::log::CARDS_GRANTED,
                        &events::CardsGrantedPayload {
                            cards: change.granted_cards.clone(),
                            source: change.source.clone(),
                        },
                    );

                    if let Ok(db) = app.state::<Mutex<HistoryDb>>().try_lock() {
                        let _ = db.log_card_grants(&change.source, &change.granted_cards);
                    }
                }
            }

            // Check for cosmetics in any change and merge
            let mut has_cosmetics = false;
            for change in &changes {
                if !change.cosmetics.art_styles.is_empty()
                    || !change.cosmetics.avatars.is_empty()
                    || !change.cosmetics.sleeves.is_empty()
                    || !change.cosmetics.pets.is_empty()
                    || !change.cosmetics.emotes.is_empty()
                    || !change.cosmetics.titles.is_empty()
                    || !change.cosmetics.other.is_empty()
                {
                    has_cosmetics = true;
                    let mut stored = cosmetics_arc.lock().unwrap();
                    stored.merge_change(&change.cosmetics);
                }
            }

            if has_cosmetics {
                let current = cosmetics_arc.lock().unwrap().clone();
                let _ = app.emit(
                    events::log::COSMETICS_UPDATED,
                    &events::CosmeticsUpdatedPayload {
                        cosmetics: current,
                    },
                );
            }

            let _ = app.emit(
                events::log::INVENTORY_UPDATED,
                &events::InventoryUpdatedPayload { changes },
            );
        }
        Err(e) => {
            diagnostics::emit_warning(
                app,
                &LOG_006,
                &format!("InventoryChange parse failed: {}", e),
            );
        }
    }
}

/// Handle a CourseList event — parse courses, store and emit.
pub fn handle_course_list(
    app: &AppHandle,
    data: &Value,
    courses_arc: &Arc<Mutex<Vec<EventCourse>>>,
) {
    // Schema observation — best-effort, never blocks parsing
    if let Ok(mut observer) = app.state::<Mutex<SchemaObserver>>().try_lock() {
        observer.check_critical("CourseList", data, app);
        observer.observe("CourseList", data, app);
        if let Some(first_course) = data
            .get("Courses")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
        {
            observer.observe("Course", first_course, app);
        }
    }

    match inventory::parse_course_list(data) {
        Ok(courses) => {
            log::info!("CourseList: {} active course(s)", courses.len());
            let _ = app.emit(
                events::log::EVENTS_UPDATED,
                &events::EventsUpdatedPayload {
                    courses: courses.clone(),
                },
            );
            *courses_arc.lock().unwrap() = courses;
        }
        Err(e) => {
            diagnostics::emit_warning(app, &LOG_009, &format!("CourseList parse failed: {}", e));
        }
    }
}

/// Handle a ProgressUpdate event — parse mastery graph state, store and emit.
pub fn handle_progress_update(
    app: &AppHandle,
    data: &Value,
    mastery_arc: &Arc<Mutex<Option<MasteryState>>>,
) {
    // Schema observation — best-effort, never blocks parsing
    if let Ok(mut observer) = app.state::<Mutex<SchemaObserver>>().try_lock() {
        observer.check_critical("ProgressUpdate", data, app);
        observer.observe("ProgressUpdate", data, app);
    }

    match inventory::parse_progress_update(data) {
        Ok(state) => {
            log::info!(
                "ProgressUpdate: {} nodes, {} milestones",
                state.node_count,
                state.milestone_count,
            );
            let _ = app.emit(
                events::log::MASTERY_UPDATED,
                &events::MasteryUpdatedPayload {
                    state: state.clone(),
                },
            );
            *mastery_arc.lock().unwrap() = Some(state);
        }
        Err(e) => {
            diagnostics::emit_warning(app, &LOG_012, &format!("ProgressUpdate parse failed: {}", e));
        }
    }
}

/// Handle a MatchGameRoomStateChanged event — capture raw JSON + defensive parse.
///
/// Three-part approach:
/// 1. Always dump raw JSON to debug log (first encounter also info)
/// 2. Defensive field extraction with fallback field names
/// 3. State tracking: Playing → store ActiveMatchState, MatchCompleted → emit MatchResult
pub fn handle_match_room_state(
    app: &AppHandle,
    data: &Value,
    match_state_arc: &Arc<Mutex<Option<ActiveMatchState>>>,
    client_id: &str,
) {
    // Part 1 — Raw capture (always)
    let event_val = data.get("matchGameRoomStateChangedEvent");
    log::debug!(
        "MatchRoomState raw: {}",
        event_val
            .map(|v| serde_json::to_string(v).unwrap_or_default())
            .unwrap_or_else(|| "null".to_string())
    );

    // Part 2 — Defensive field extraction
    let event = match event_val {
        Some(v) => v,
        None => {
            diagnostics::emit_warning(
                app,
                &LOG_018,
                "Missing matchGameRoomStateChangedEvent key",
            );
            return;
        }
    };

    // Try alternate casing for gameRoomInfo
    let game_room_info = event
        .get("gameRoomInfo")
        .or_else(|| event.get("GameRoomInfo"))
        .or_else(|| event.get("game_room_info"));

    let game_room_info = match game_room_info {
        Some(v) => v,
        None => {
            diagnostics::emit_warning(app, &LOG_018, "Missing gameRoomInfo — check raw log");
            return;
        }
    };

    // Extract stateType with fallback casing
    let state_type = game_room_info
        .get("stateType")
        .or_else(|| game_room_info.get("StateType"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Extract gameRoomConfig with fallback casing
    let config = game_room_info
        .get("gameRoomConfig")
        .or_else(|| game_room_info.get("GameRoomConfig"));

    // Part 3 — State + emit
    if state_type.contains("Playing") {
        // Match started — extract player info and store state
        let match_id = config
            .and_then(|c| c.get("matchId").or_else(|| c.get("MatchId")))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        // eventId lives on each reservedPlayer, not on gameRoomConfig directly
        let event_id = config
            .and_then(|c| {
                c.get("reservedPlayers")
                    .or_else(|| c.get("ReservedPlayers"))
            })
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|p| p.get("eventId").or_else(|| p.get("EventId")))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let players = config
            .and_then(|c| {
                c.get("reservedPlayers")
                    .or_else(|| c.get("ReservedPlayers"))
            })
            .and_then(|v| v.as_array());

        if let Some(players) = players {
            // Find local player by matching userId against clientId from log header.
            // The reservedPlayers array is ordered by systemSeatId, NOT local/remote.
            let (local_idx, opponent_idx) = if !client_id.is_empty() {
                let idx = players.iter().position(|p| {
                    p.get("userId")
                        .and_then(|v| v.as_str())
                        .map(|uid| uid == client_id)
                        .unwrap_or(false)
                });
                match idx {
                    Some(i) => {
                        let opp = if i == 0 { 1 } else { 0 };
                        (i, opp)
                    }
                    None => {
                        log::warn!("clientId {} not found in reservedPlayers, falling back to index 0", client_id);
                        (0, 1)
                    }
                }
            } else {
                log::warn!("No clientId available — falling back to index 0 for local player");
                (0, 1)
            };

            let our_team_id = players
                .get(local_idx)
                .and_then(|p| p.get("teamId").or_else(|| p.get("TeamId")))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let our_name = players
                .get(local_idx)
                .and_then(|p| {
                    p.get("playerName")
                        .or_else(|| p.get("PlayerName"))
                })
                .and_then(|v| v.as_str())
                .unwrap_or("You");
            let opponent_name = players
                .get(opponent_idx)
                .and_then(|p| {
                    p.get("playerName")
                        .or_else(|| p.get("PlayerName"))
                })
                .and_then(|v| v.as_str())
                .unwrap_or("Opponent")
                .to_string();

            log::info!("Match started: {} vs {} (clientId={})", our_name, opponent_name, client_id);

            *match_state_arc.lock().unwrap() = Some(ActiveMatchState {
                match_id,
                event_id,
                our_team_id,
                opponent_name,
            });
        } else {
            diagnostics::emit_warning(app, &LOG_018, "Playing state but no reservedPlayers");
        }
    } else if state_type.contains("MatchCompleted") {
        // Match completed — determine outcome and emit result
        let stored = match_state_arc.lock().unwrap().take();

        let final_result = game_room_info
            .get("finalMatchResult")
            .or_else(|| game_room_info.get("FinalMatchResult"));

        let completed_reason = final_result
            .and_then(|r| {
                r.get("matchCompletedReason")
                    .or_else(|| r.get("MatchCompletedReason"))
            })
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();

        let result_list = final_result
            .and_then(|r| r.get("resultList").or_else(|| r.get("ResultList")))
            .and_then(|v| v.as_array());

        // Resolve our_team_id: from stored state, or derive from clientId
        let our_team_id: Option<i64> = match stored {
            Some(ref s) => Some(s.our_team_id),
            None => {
                // Derive from reservedPlayers using clientId
                if !client_id.is_empty() {
                    config
                        .and_then(|c| {
                            c.get("reservedPlayers")
                                .or_else(|| c.get("ReservedPlayers"))
                        })
                        .and_then(|v| v.as_array())
                        .and_then(|arr| {
                            arr.iter().find(|p| {
                                p.get("userId").and_then(|v| v.as_str()) == Some(client_id)
                            })
                        })
                        .and_then(|p| p.get("teamId").or_else(|| p.get("TeamId")))
                        .and_then(|v| v.as_i64())
                } else {
                    None
                }
            }
        };

        // Find the Match-scope result (not Game-scope)
        let outcome = if let Some(results) = result_list {
            let match_result = results.iter().find(|r| {
                r.get("scope")
                    .or_else(|| r.get("Scope"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.contains("Match"))
                    .unwrap_or(false)
            });

            if let Some(result_entry) = match_result {
                let winning_team = result_entry
                    .get("winningTeamId")
                    .or_else(|| result_entry.get("WinningTeamId"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);

                if winning_team == 0 {
                    MatchOutcome::Draw
                } else if let Some(team_id) = our_team_id {
                    if winning_team == team_id {
                        MatchOutcome::Victory
                    } else {
                        MatchOutcome::Defeat
                    }
                } else {
                    MatchOutcome::Unknown
                }
            } else {
                MatchOutcome::Unknown
            }
        } else {
            MatchOutcome::Unknown
        };

        // Extract match metadata
        let players_arr = config
            .and_then(|c| {
                c.get("reservedPlayers")
                    .or_else(|| c.get("ReservedPlayers"))
            })
            .and_then(|v| v.as_array());

        let (local_idx, opponent_idx) = if let Some(arr) = players_arr {
            if !client_id.is_empty() {
                let idx = arr.iter().position(|p| {
                    p.get("userId").and_then(|v| v.as_str()) == Some(client_id)
                });
                match idx {
                    Some(i) => (i, if i == 0 { 1 } else { 0 }),
                    None => (0, 1),
                }
            } else {
                (0, 1)
            }
        } else {
            (0, 1)
        };

        let (match_id, event_id, opponent_name) = match stored {
            Some(ref s) => (s.match_id.clone(), s.event_id.clone(), s.opponent_name.clone()),
            None => {
                let mid = config
                    .and_then(|c| c.get("matchId").or_else(|| c.get("MatchId")))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let eid = players_arr
                    .and_then(|arr| arr.get(local_idx))
                    .and_then(|p| p.get("eventId").or_else(|| p.get("EventId")))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let opp = players_arr
                    .and_then(|arr| arr.get(opponent_idx))
                    .and_then(|p| p.get("playerName").or_else(|| p.get("PlayerName")))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Opponent")
                    .to_string();
                (mid, eid, opp)
            }
        };

        let match_result = MatchResult {
            match_id,
            event_id,
            opponent_name: opponent_name.clone(),
            result: outcome,
            completed_reason,
            game_mode: None,
        };

        log::info!(
            "Match completed: {:?} vs {} ({})",
            match_result.result,
            opponent_name,
            match_result.completed_reason,
        );

        let _ = app.emit(
            events::log::MATCH_RESULT,
            &events::MatchResultPayload {
                result: match_result,
            },
        );
    }
    // Other state types (e.g., Connecting, Finalizing) — silently ignored
}

/// Handle HomePageAchievements extraction from StartHook — discovery-first.
///
/// Emits the raw JSON structure and logs its shape for discovery.
/// Proper parsing will follow once we verify the live structure.
pub fn handle_achievements(
    app: &AppHandle,
    data: &Value,
    achievements_arc: &Arc<Mutex<Option<Value>>>,
    is_refresh: bool,
) {
    let achievements = data
        .get("HomePageAchievements")
        .or_else(|| {
            data.get("InventoryInfo")
                .and_then(|v| v.get("HomePageAchievements"))
        });

    let achievements = match achievements {
        Some(v) => v.clone(),
        None => {
            log::debug!("StartHook: no HomePageAchievements found");
            return;
        }
    };

    // Discovery logging — log shape at info level
    if let Some(obj) = achievements.as_object() {
        log::info!(
            "HomePageAchievements: {} keys — {:?}",
            obj.len(),
            obj.keys().collect::<Vec<_>>(),
        );
    } else if let Some(arr) = achievements.as_array() {
        let first_keys: Vec<String> = arr.first()
            .and_then(|v| v.as_object())
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default();
        log::info!(
            "HomePageAchievements: array of {} items, first keys: {:?}",
            arr.len(),
            first_keys,
        );
    } else {
        log::info!("HomePageAchievements: unexpected type — {}", achievements);
    }

    let _ = app.emit(
        events::log::ACHIEVEMENTS_UPDATED,
        &events::AchievementsUpdatedPayload {
            achievements: achievements.clone(),
            is_refresh,
        },
    );

    *achievements_arc.lock().unwrap() = Some(achievements);
}
