// LogService — tails Player.log, classifies events, parses inventory.
//
// Orchestrates the log watching pipeline: find log → initial scan →
// tail for changes → classify and dispatch events.
// Emits typed events via Tauri's event system.

pub mod decks;
pub mod handlers;
pub mod inventory;
pub mod parser;
pub mod schema;
pub mod watcher;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::events;
use crate::models::inventory::{DeckContents, PlayerCosmetics, PlayerInventory};
use crate::models::{ActiveMatchState, EventCourse, MasteryState};
use crate::services::diagnostics::{self, LOG_001};

/// Managed state for the log watcher.
pub struct LogService {
    running: bool,
    inventory: Arc<Mutex<Option<PlayerInventory>>>,
    non_collectible_ids: Arc<Mutex<HashSet<i32>>>,
    anchor_ids: Arc<Mutex<HashSet<i32>>>,
    event_courses: Arc<Mutex<Vec<EventCourse>>>,
    cosmetics: Arc<Mutex<PlayerCosmetics>>,
    mastery: Arc<Mutex<Option<MasteryState>>>,
    decks: Arc<Mutex<HashMap<String, DeckContents>>>,
    match_state: Arc<Mutex<Option<ActiveMatchState>>>,
    achievements: Arc<Mutex<Option<serde_json::Value>>>,
    stop_flag: Arc<AtomicBool>,
    watcher_thread: Option<JoinHandle<()>>,
    log_path: Option<PathBuf>,
}

/// Status snapshot for frontend queries.
#[derive(Debug, Clone, Serialize)]
pub struct LogStatus {
    pub running: bool,
    pub log_path: Option<String>,
    pub has_inventory: bool,
}

impl LogService {
    pub fn new() -> Self {
        Self {
            running: false,
            inventory: Arc::new(Mutex::new(None)),
            non_collectible_ids: Arc::new(Mutex::new(HashSet::new())),
            anchor_ids: Arc::new(Mutex::new(HashSet::new())),
            event_courses: Arc::new(Mutex::new(Vec::new())),
            cosmetics: Arc::new(Mutex::new(PlayerCosmetics::default())),
            mastery: Arc::new(Mutex::new(None)),
            decks: Arc::new(Mutex::new(HashMap::new())),
            match_state: Arc::new(Mutex::new(None)),
            achievements: Arc::new(Mutex::new(None)),
            stop_flag: Arc::new(AtomicBool::new(false)),
            watcher_thread: None,
            log_path: None,
        }
    }

    /// Start the log watcher. Finds Player.log, runs initial scan synchronously
    /// (so anchor_ids are populated before returning), then spawns a background
    /// thread for live watching.
    pub fn start(&mut self, app: &AppHandle) -> Result<String, String> {
        if self.running {
            return Err("Log watcher is already running".to_string());
        }

        let log_path = watcher::find_player_log().map_err(|e| {
            diagnostics::emit_error(app, &LOG_001, &e);
            e
        })?;

        let path_str = log_path.display().to_string();
        log::info!("Starting log watcher on {}", path_str);

        self.stop_flag.store(false, Ordering::Relaxed);

        // Run initial scan synchronously
        let position = watcher::initial_scan(
            app,
            &log_path,
            &self.inventory,
            &self.non_collectible_ids,
            &self.anchor_ids,
            &self.event_courses,
            &self.cosmetics,
            &self.mastery,
            &self.decks,
            &self.achievements,
        ).map_err(|e| {
            diagnostics::emit_error(app, &LOG_001, &e);
            e
        })?;

        // Spawn thread for live watching only (initial scan already done)
        let app_clone = app.clone();
        let path_clone = log_path.clone();
        let stop = self.stop_flag.clone();
        let inv = self.inventory.clone();
        let nc = self.non_collectible_ids.clone();
        let anc = self.anchor_ids.clone();
        let courses = self.event_courses.clone();
        let cosmetics = self.cosmetics.clone();
        let mastery = self.mastery.clone();
        let decks = self.decks.clone();
        let match_state = self.match_state.clone();
        let achievements = self.achievements.clone();

        let handle = std::thread::Builder::new()
            .name("log-watcher".to_string())
            .spawn(move || {
                watcher::run_live_watch(
                    app_clone, path_clone, stop, inv, nc, anc, courses,
                    cosmetics, mastery, decks, match_state, achievements,
                    position,
                );
            })
            .map_err(|e| format!("Failed to spawn watcher thread: {}", e))?;

        self.watcher_thread = Some(handle);
        self.log_path = Some(log_path);
        self.running = true;

        let _ = app.emit(
            events::log::WATCHER_STARTED,
            &events::WatcherStartedPayload {
                log_path: path_str.clone(),
            },
        );

        Ok(path_str)
    }

    /// Stop the log watcher.
    pub fn stop(&mut self) {
        if !self.running {
            return;
        }

        log::info!("Stopping log watcher");
        self.stop_flag.store(true, Ordering::Relaxed);

        if let Some(handle) = self.watcher_thread.take() {
            let _ = handle.join();
        }

        self.running = false;
    }

    /// Get the current inventory snapshot, if available.
    pub fn current_inventory(&self) -> Option<PlayerInventory> {
        self.inventory.lock().ok()?.clone()
    }

    /// Get non-collectible card IDs (for MemoryService integration).
    pub fn non_collectible_ids(&self) -> HashSet<i32> {
        self.non_collectible_ids
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// Get anchor card IDs from user decks (for MemoryService integration).
    pub fn anchor_ids(&self) -> HashSet<i32> {
        self.anchor_ids
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// Get the current list of active event courses.
    pub fn current_courses(&self) -> Vec<EventCourse> {
        self.event_courses
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// Get the current cosmetics state.
    pub fn current_cosmetics(&self) -> PlayerCosmetics {
        self.cosmetics
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// Get the current mastery state, if available.
    pub fn current_mastery(&self) -> Option<MasteryState> {
        self.mastery.lock().ok()?.clone()
    }

    /// Get cached deck card lists from the last StartHook.
    pub fn current_decks(&self) -> HashMap<String, DeckContents> {
        self.decks
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// Check if the watcher is running.
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Get status snapshot for frontend.
    pub fn status(&self) -> LogStatus {
        LogStatus {
            running: self.running,
            log_path: self.log_path.as_ref().map(|p| p.display().to_string()),
            has_inventory: self
                .inventory
                .lock()
                .map(|g| g.is_some())
                .unwrap_or(false),
        }
    }
}

impl Drop for LogService {
    fn drop(&mut self) {
        self.stop();
    }
}
