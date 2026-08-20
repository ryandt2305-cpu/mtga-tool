// File watcher for Player.log — notify setup, read loop, rotation detection.
//
// Spawned on a background thread by LogService. Uses the `notify` crate for
// filesystem events and falls back to polling if needed.
// Handler bodies are in handlers.rs.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter};

use crate::events;
use crate::models::inventory::{DeckContents, PlayerCosmetics, PlayerInventory};
use crate::models::{ActiveMatchState, EventCourse, MasteryState};
use crate::services::diagnostics::{self, LOG_002, LOG_003, LOG_004, LOG_007, LOG_008, LOG_019};
use crate::services::log::handlers;
use crate::services::log::parser::{self, LogEventType};

/// Find Player.log at the standard MTGA path.
///
/// Path: `%LOCALAPPDATA%/../LocalLow/Wizards Of The Coast/MTGA/Player.log`
pub fn find_player_log() -> Result<PathBuf, String> {
    let local_app = std::env::var("LOCALAPPDATA")
        .map_err(|_| "LOCALAPPDATA environment variable not set".to_string())?;

    let local_low = Path::new(&local_app)
        .parent()
        .ok_or_else(|| "Cannot resolve LocalLow from LOCALAPPDATA".to_string())?
        .join("LocalLow");

    let log_path = local_low
        .join("Wizards Of The Coast")
        .join("MTGA")
        .join("Player.log");

    if log_path.exists() {
        Ok(log_path)
    } else {
        Err(format!("Player.log not found at {}", log_path.display()))
    }
}

/// Live watch loop — runs on a background thread after initial_scan completes.
pub(super) fn run_live_watch(
    app: AppHandle,
    log_path: PathBuf,
    stop_flag: Arc<AtomicBool>,
    inventory_arc: Arc<Mutex<Option<PlayerInventory>>>,
    non_collectible_arc: Arc<Mutex<HashSet<i32>>>,
    anchor_arc: Arc<Mutex<HashSet<i32>>>,
    courses_arc: Arc<Mutex<Vec<EventCourse>>>,
    cosmetics_arc: Arc<Mutex<PlayerCosmetics>>,
    mastery_arc: Arc<Mutex<Option<MasteryState>>>,
    decks_arc: Arc<Mutex<HashMap<String, DeckContents>>>,
    match_state_arc: Arc<Mutex<Option<ActiveMatchState>>>,
    achievements_arc: Arc<Mutex<Option<serde_json::Value>>>,
    mut position: u64,
) {
    let (tx, rx) = mpsc::channel();
    let _watcher = match setup_watcher(&log_path, tx) {
        Ok(w) => w,
        Err(e) => {
            let msg = format!("File watcher setup failed: {}", e);
            diagnostics::emit_error(&app, &LOG_003, &msg);
            let _ = app.emit(
                events::log::WATCHER_STOPPED,
                &events::WatcherStoppedPayload { reason: msg },
            );
            return;
        }
    };

    log::info!(
        "Log watcher running — watching {} from position {}",
        log_path.display(),
        position
    );

    let mut line_buffer = String::new();
    let mut client_id = String::new();

    loop {
        if stop_flag.load(Ordering::Relaxed) {
            log::info!("Log watcher stop requested");
            let _ = app.emit(
                events::log::WATCHER_STOPPED,
                &events::WatcherStoppedPayload {
                    reason: "Stopped by user".to_string(),
                },
            );
            return;
        }

        // Drain notify events (rotation detection)
        loop {
            match rx.try_recv() {
                Ok(Ok(event)) => {
                    let dominated = event.paths.iter().any(|p| {
                        p.file_name().map_or(false, |n| n == "Player.log")
                    });
                    if !dominated {
                        continue;
                    }
                    if matches!(event.kind, EventKind::Create(_)) {
                        diagnostics::emit_info(&app, &LOG_007, "Player.log rotated (created)");
                        position = match initial_scan(
                            &app, &log_path, &inventory_arc, &non_collectible_arc,
                            &anchor_arc, &courses_arc, &cosmetics_arc, &mastery_arc,
                            &decks_arc, &achievements_arc,
                        ) {
                            Ok(pos) => pos,
                            Err(e) => {
                                diagnostics::emit_warning(&app, &LOG_007, &format!(
                                    "Rotation re-scan failed: {}", e
                                ));
                                0
                            }
                        };
                        line_buffer.clear();
                    }
                }
                Ok(Err(e)) => {
                    let msg = format!("File watcher error: {}", e);
                    diagnostics::emit_error(&app, &LOG_008, &msg);
                    let _ = app.emit(
                        events::log::WATCHER_STOPPED,
                        &events::WatcherStoppedPayload { reason: msg },
                    );
                    return;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    let msg = "File watcher channel disconnected";
                    diagnostics::emit_error(&app, &LOG_008, msg);
                    let _ = app.emit(
                        events::log::WATCHER_STOPPED,
                        &events::WatcherStoppedPayload {
                            reason: msg.to_string(),
                        },
                    );
                    return;
                }
            }
        }

        // Poll: check file size and read new data
        let current_size = std::fs::metadata(&log_path)
            .map(|m| m.len())
            .unwrap_or(position);

        if current_size < position {
            diagnostics::emit_info(&app, &LOG_007, "Player.log rotated (file shrunk)");
            position = match initial_scan(
                &app, &log_path, &inventory_arc, &non_collectible_arc,
                &anchor_arc, &courses_arc, &cosmetics_arc, &mastery_arc,
                &decks_arc, &achievements_arc,
            ) {
                Ok(pos) => pos,
                Err(e) => {
                    diagnostics::emit_warning(&app, &LOG_007, &format!(
                        "Rotation re-scan failed: {}", e
                    ));
                    0
                }
            };
            line_buffer.clear();
        } else if current_size > position {
            log::debug!(
                "Log grew: {} → {} (+{} bytes)",
                position, current_size, current_size - position
            );
            match read_new_lines(&log_path, position, &mut line_buffer) {
                Ok((lines, new_pos)) => {
                    log::debug!("Read {} new lines", lines.len());
                    position = new_pos;
                    for line in lines {
                        process_line(
                            &app, &line, &inventory_arc, &non_collectible_arc,
                            &anchor_arc, &courses_arc, &cosmetics_arc, &mastery_arc,
                            &decks_arc, &match_state_arc, &achievements_arc,
                            &mut client_id,
                        );
                    }
                }
                Err(e) => {
                    diagnostics::emit_warning(&app, &LOG_002, &format!(
                        "Failed to read new log data: {}", e
                    ));
                }
            }
        }

        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Read the full file, parse all lines, keep the last StartHook/CourseList/ProgressUpdate.
/// Returns the file position at EOF.
pub(super) fn initial_scan(
    app: &AppHandle,
    log_path: &Path,
    inventory_arc: &Arc<Mutex<Option<PlayerInventory>>>,
    non_collectible_arc: &Arc<Mutex<HashSet<i32>>>,
    anchor_arc: &Arc<Mutex<HashSet<i32>>>,
    courses_arc: &Arc<Mutex<Vec<EventCourse>>>,
    cosmetics_arc: &Arc<Mutex<PlayerCosmetics>>,
    mastery_arc: &Arc<Mutex<Option<MasteryState>>>,
    decks_arc: &Arc<Mutex<HashMap<String, DeckContents>>>,
    achievements_arc: &Arc<Mutex<Option<serde_json::Value>>>,
) -> Result<u64, String> {
    let file = File::open(log_path)
        .map_err(|e| format!("Failed to open {}: {}", log_path.display(), e))?;

    let reader = BufReader::new(&file);
    let mut last_start_hook: Option<serde_json::Value> = None;
    let mut last_course_list: Option<serde_json::Value> = None;
    let mut last_progress_update: Option<serde_json::Value> = None;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };

        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }

        // Quick pre-check: only parse lines that might be relevant
        let has_inventory = trimmed.contains("InventoryInfo");
        let has_courses = trimmed.contains("\"Courses\"");
        let has_nodes = trimmed.contains("\"NodeStates\"");
        if !has_inventory && !has_courses && !has_nodes {
            continue;
        }

        match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(data) => {
                let event_type = parser::classify(&data);
                match event_type {
                    LogEventType::StartHook => {
                        last_start_hook = Some(data);
                    }
                    LogEventType::CourseList => {
                        last_course_list = Some(data);
                    }
                    LogEventType::ProgressUpdate => {
                        last_progress_update = Some(data);
                    }
                    _ => {}
                }
            }
            Err(e) => {
                diagnostics::emit_warning(
                    app,
                    &LOG_004,
                    &format!("JSON parse error in initial scan: {}", e),
                );
            }
        }
    }

    // Parse the last CourseList found
    if let Some(data) = last_course_list {
        handlers::handle_course_list(app, &data, courses_arc);
    }

    // Parse the last StartHook found
    if let Some(data) = last_start_hook {
        handlers::handle_start_hook(
            app, &data, inventory_arc, non_collectible_arc,
            anchor_arc, courses_arc, cosmetics_arc, mastery_arc, decks_arc,
            achievements_arc,
        );
    } else {
        log::info!("No StartHook found in log (MTGA may not have completed login)");
    }

    // Parse the last ProgressUpdate found
    if let Some(data) = last_progress_update {
        handlers::handle_progress_update(app, &data, mastery_arc);
    }

    let size = std::fs::metadata(log_path)
        .map(|m| m.len())
        .map_err(|e| format!("Failed to stat {}: {}", log_path.display(), e))?;

    // Detailed Logs heuristic: if the file has substantial content but no
    // StartHook was ever parsed, "Detailed Logs (Plugin Support)" is almost
    // certainly disabled in MTGA — the log gets short status lines but none
    // of the InventoryInfo/DeckSummaries JSON blobs. Warn the user so they
    // don't sit staring at an empty economy view.
    //
    // 128 KiB threshold picks up a running session that's had time to write
    // some traffic, while skipping a freshly-truncated log where the user is
    // still on the MTGA login screen.
    const DETAILED_LOGS_HEURISTIC_MIN_SIZE: u64 = 128 * 1024;
    if inventory_arc.lock().map(|g| g.is_none()).unwrap_or(true)
        && size >= DETAILED_LOGS_HEURISTIC_MIN_SIZE
    {
        diagnostics::emit_warning(
            app,
            &LOG_019,
            "Player.log has no StartHook — MTGA's \"Detailed Logs (Plugin Support)\" \
             option is likely disabled. Enable it under Options → Account, then fully \
             restart MTGA. Without it, wildcards, gold, gems, and boosters cannot be read.",
        );
    }

    Ok(size)
}

/// Read new data from `position` to EOF, splitting into complete lines.
fn read_new_lines(
    log_path: &Path,
    position: u64,
    line_buffer: &mut String,
) -> Result<(Vec<String>, u64), String> {
    let mut file = File::open(log_path)
        .map_err(|e| format!("Failed to open {}: {}", log_path.display(), e))?;

    file.seek(SeekFrom::Start(position))
        .map_err(|e| format!("Failed to seek: {}", e))?;

    let mut new_data = String::new();
    file.read_to_string(&mut new_data)
        .map_err(|e| format!("Failed to read: {}", e))?;

    let new_position = position + new_data.len() as u64;

    if !line_buffer.is_empty() {
        new_data.insert_str(0, line_buffer);
        line_buffer.clear();
    }

    let mut lines = Vec::new();
    let mut last_newline = 0;

    for (i, ch) in new_data.char_indices() {
        if ch == '\n' {
            let line = &new_data[last_newline..i];
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                lines.push(trimmed.to_string());
            }
            last_newline = i + 1;
        }
    }

    if last_newline < new_data.len() {
        line_buffer.push_str(&new_data[last_newline..]);
    }

    Ok((lines, new_position))
}

/// Process a single complete line from the tail — classify and dispatch to handlers.
fn process_line(
    app: &AppHandle,
    line: &str,
    inventory_arc: &Arc<Mutex<Option<PlayerInventory>>>,
    non_collectible_arc: &Arc<Mutex<HashSet<i32>>>,
    anchor_arc: &Arc<Mutex<HashSet<i32>>>,
    courses_arc: &Arc<Mutex<Vec<EventCourse>>>,
    cosmetics_arc: &Arc<Mutex<PlayerCosmetics>>,
    mastery_arc: &Arc<Mutex<Option<MasteryState>>>,
    decks_arc: &Arc<Mutex<HashMap<String, DeckContents>>>,
    match_state_arc: &Arc<Mutex<Option<ActiveMatchState>>>,
    achievements_arc: &Arc<Mutex<Option<serde_json::Value>>>,
    client_id: &mut String,
) {
    if !line.starts_with('{') {
        // Extract local player's userId from "Match to {userId}" header lines.
        // These appear before every MatchGameRoomStateChanged JSON event.
        if let Some(pos) = line.find("Match to ") {
            let after = &line[pos + 9..];
            if let Some(end) = after.find(':') {
                *client_id = after[..end].trim().to_string();
                log::debug!("Extracted clientId from header: {}", client_id);
            }
        }
        return;
    }

    // Skip lines too short to be complete JSON events — these are fragments
    // from multi-line pretty-printed JSON blobs in Player.log (e.g. just "{").
    if line.len() < 10 {
        return;
    }

    let data: serde_json::Value = match serde_json::from_str(line) {
        Ok(d) => d,
        Err(e) => {
            let msg = e.to_string();
            // "EOF while parsing" = multi-line JSON fragment, expected noise
            if msg.contains("EOF while parsing") {
                log::debug!("Multi-line JSON fragment in live watch ({} bytes)", line.len());
            } else {
                diagnostics::emit_warning(
                    app,
                    &LOG_004,
                    &format!("JSON parse error in live watch: {}", e),
                );
            }
            return;
        }
    };

    let event_type = parser::classify(&data);
    let raw_size = line.len();

    match event_type {
        LogEventType::StartHook => {
            handlers::handle_start_hook(
                app, &data, inventory_arc, non_collectible_arc,
                anchor_arc, courses_arc, cosmetics_arc, mastery_arc, decks_arc,
                achievements_arc,
            );
        }
        LogEventType::InventoryUpdate => {
            handlers::handle_inventory_update(app, &data, cosmetics_arc, courses_arc);
        }
        LogEventType::CourseList => {
            handlers::handle_course_list(app, &data, courses_arc);
        }
        LogEventType::ProgressUpdate => {
            handlers::handle_progress_update(app, &data, mastery_arc);
        }
        LogEventType::MatchRoomState => {
            handlers::handle_match_room_state(app, &data, match_state_arc, client_id);
        }
        // High-volume noise — log only
        LogEventType::ApiResponse
        | LogEventType::GameEvent
        | LogEventType::RewardChestConfig
        | LogEventType::FormatList
        | LogEventType::PreconDecks
        | LogEventType::DeckSummaries
        | LogEventType::MatchHistory => {
            log::trace!("{} ({} bytes)", event_type.as_str(), raw_size);
        }
        LogEventType::Unknown => {
            let keys: Vec<&str> = data
                .as_object()
                .map(|o| o.keys().map(|k| k.as_str()).collect())
                .unwrap_or_default();
            log::debug!("Unknown JSON event ({} bytes), keys: {:?}", raw_size, keys);
        }
        LogEventType::ModulePayload => {
            if let Some(module) = data.get("CurrentModule").and_then(|v| v.as_str()) {
                if module.starts_with("Play_") {
                    let game_mode = module
                        .strip_prefix("Play_")
                        .unwrap_or(module)
                        .to_string();
                    let _ = app.emit(
                        events::log::MATCH_STARTING,
                        &events::MatchStartingPayload { game_mode },
                    );
                }
            }
            // All ModulePayloads suppressed from LOG_EVENT — they're internal game state
        }
        LogEventType::DeckUpdate => {
            let summary = parser::summarize(&event_type, &data);
            let mut details = parser::extract_details(&event_type, &data);
            // Enrich with cached deck card list from StartHook
            if let Some(deck_id) = data.get("DeckId").and_then(|v| v.as_str()) {
                if let Ok(decks) = decks_arc.lock() {
                    if let Some(contents) = decks.get(deck_id) {
                        if let Some(ref mut d) = details {
                            d["mainboard"] = serde_json::to_value(&contents.mainboard)
                                .unwrap_or_default();
                            d["sideboard"] = serde_json::to_value(&contents.sideboard)
                                .unwrap_or_default();
                        }
                    }
                }
            }
            let _ = app.emit(
                events::log::LOG_EVENT,
                &events::LogEventPayload {
                    event_type: event_type.as_str().to_string(),
                    summary,
                    raw_size,
                    details,
                },
            );
        }
        _ => {
            let summary = parser::summarize(&event_type, &data);
            let details = parser::extract_details(&event_type, &data);
            let _ = app.emit(
                events::log::LOG_EVENT,
                &events::LogEventPayload {
                    event_type: event_type.as_str().to_string(),
                    summary,
                    raw_size,
                    details,
                },
            );
        }
    }
}

/// Set up the notify watcher on the log file's parent directory.
fn setup_watcher(
    log_path: &Path,
    tx: mpsc::Sender<Result<Event, notify::Error>>,
) -> Result<RecommendedWatcher, String> {
    let mut watcher = RecommendedWatcher::new(
        move |res| {
            let _ = tx.send(res);
        },
        Config::default().with_poll_interval(Duration::from_secs(1)),
    )
    .map_err(|e| format!("Failed to create watcher: {}", e))?;

    let parent = log_path
        .parent()
        .ok_or_else(|| "Log path has no parent directory".to_string())?;

    watcher
        .watch(parent, RecursiveMode::NonRecursive)
        .map_err(|e| format!("Failed to watch {}: {}", parent.display(), e))?;

    Ok(watcher)
}
