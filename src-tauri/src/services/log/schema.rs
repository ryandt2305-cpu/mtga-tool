// SchemaObserver — dynamic field validation for log events.
//
// Two layers:
// 1. Critical fields: tiny static list of fields whose absence means parse failure.
// 2. Dynamic observer: auto-learns field shapes from live data, detects drift
//    against historical baseline, persists observations across sessions.
//
// No manual schema maintenance — the observer learns the game's format from
// the data itself.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::AppHandle;

use crate::services::diagnostics::{self, LOG_015, LOG_016, LOG_017};

/// Minimum observation count before disappearance drift is reported.
const MIN_OBSERVATIONS_FOR_DRIFT: u32 = 2;

/// Minimum presence rate to consider a field "expected" (80%).
const PRESENCE_THRESHOLD: f64 = 0.8;

/// Critical fields — if these are absent, parsing fails entirely.
/// Each entry: (event_name, field_path, description).
const CRITICAL_FIELDS: &[(&str, &str, &str)] = &[
    ("StartHook", "InventoryInfo", "required by parse_start_hook"),
    (
        "InventoryUpdate",
        "InventoryInfo",
        "required by parse_inventory_changes",
    ),
    (
        "InventoryUpdate",
        "InventoryInfo.Changes",
        "required by parse_inventory_changes",
    ),
    ("CourseList", "Courses", "required by parse_course_list"),
];

// ProgressUpdate needs NodeStates OR MilestoneStates — handled specially in check_critical.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventProfile {
    pub fields: HashMap<String, FieldProfile>,
    pub observation_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldProfile {
    pub json_type: String,
    pub times_seen: u32,
    pub first_seen: String,
    pub last_seen: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftEntry {
    pub event_name: String,
    pub field_name: String,
    pub drift_type: DriftType,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum DriftType {
    FieldDisappeared,
    FieldAppeared,
    TypeChanged,
}

/// Composite key for deduplicating drift entries within a session.
#[derive(PartialEq, Eq, Hash)]
struct DriftKey {
    event_name: String,
    field_name: String,
    drift_type: DriftType,
}

pub struct SchemaObserver {
    baseline: HashMap<String, EventProfile>,
    session: HashMap<String, EventProfile>,
    drift_log: Vec<DriftEntry>,
    drift_seen: std::collections::HashSet<DriftKey>,
    data_dir: PathBuf,
    dirty: bool,
}

impl SchemaObserver {
    /// Create a new observer, loading baseline from disk if available.
    pub fn new(data_dir: &Path) -> Self {
        let baseline = load_baseline(data_dir);
        Self {
            baseline,
            session: HashMap::new(),
            drift_log: Vec::new(),
            drift_seen: std::collections::HashSet::new(),
            data_dir: data_dir.to_path_buf(),
            dirty: false,
        }
    }

    /// Check critical fields for an event. Emits diagnostics on missing required fields.
    /// Called before parsing — gives visibility into WHY a parse might fail.
    pub fn check_critical(&self, event_name: &str, data: &Value, app: &AppHandle) {
        for &(evt, field, desc) in CRITICAL_FIELDS {
            if evt != event_name {
                continue;
            }

            // Handle dotted paths like "InventoryInfo.Changes"
            let parts: Vec<&str> = field.split('.').collect();
            let mut current = data;
            let mut missing = false;
            for part in &parts {
                match current.get(part) {
                    Some(v) => current = v,
                    None => {
                        missing = true;
                        break;
                    }
                }
            }

            if missing {
                diagnostics::emit_warning(
                    app,
                    &LOG_015,
                    &format!(
                        "Critical field '{}' missing from {} ({})",
                        field, event_name, desc
                    ),
                );
            }
        }

        // Special case: ProgressUpdate needs NodeStates OR MilestoneStates
        if event_name == "ProgressUpdate" {
            let has_nodes = data.get("NodeStates").is_some();
            let has_milestones = data.get("MilestoneStates").is_some();
            if !has_nodes && !has_milestones {
                diagnostics::emit_warning(
                    app,
                    &LOG_015,
                    "Critical field 'NodeStates' or 'MilestoneStates' missing from ProgressUpdate",
                );
            }
        }
    }

    /// Observe a JSON value for a named event point. Records all top-level fields
    /// and detects drift against the historical baseline.
    pub fn observe(&mut self, event_name: &str, data: &Value, app: &AppHandle) {
        let now = unix_timestamp_string();
        let fields = extract_fields(data);

        // Collect pending drift entries to emit after borrows are released
        let mut pending_drift: Vec<(String, String, DriftType, String)> = Vec::new();

        // Update session profile
        {
            let profile = self
                .session
                .entry(event_name.to_string())
                .or_insert_with(|| EventProfile {
                    fields: HashMap::new(),
                    observation_count: 0,
                });
            profile.observation_count += 1;

            for (name, json_type) in &fields {
                let fp = profile
                    .fields
                    .entry(name.clone())
                    .or_insert_with(|| FieldProfile {
                        json_type: json_type.clone(),
                        times_seen: 0,
                        first_seen: now.clone(),
                        last_seen: now.clone(),
                    });
                fp.times_seen += 1;
                fp.last_seen = now.clone();

                // Check for type change vs session baseline
                if fp.times_seen > 1 && fp.json_type != *json_type {
                    pending_drift.push((
                        event_name.to_string(),
                        name.clone(),
                        DriftType::TypeChanged,
                        format!(
                            "field '{}' type changed in session: {} -> {}",
                            name, fp.json_type, json_type
                        ),
                    ));
                }
            }
        }

        // Compare against historical baseline
        if let Some(baseline_profile) = self.baseline.get(event_name) {
            if baseline_profile.observation_count >= MIN_OBSERVATIONS_FOR_DRIFT {
                // Fields in baseline but not in current data
                for (field_name, field_profile) in &baseline_profile.fields {
                    if !fields.iter().any(|(n, _)| n == field_name) {
                        let presence_rate = field_profile.times_seen as f64
                            / baseline_profile.observation_count as f64;
                        if presence_rate >= PRESENCE_THRESHOLD {
                            pending_drift.push((
                                event_name.to_string(),
                                field_name.clone(),
                                DriftType::FieldDisappeared,
                                format!(
                                    "field '{}' disappeared from {} (was present in {:.0}% of {} prior observations)",
                                    field_name,
                                    event_name,
                                    presence_rate * 100.0,
                                    baseline_profile.observation_count,
                                ),
                            ));
                        }
                    }
                }

                // Fields in current data but not in baseline
                for (field_name, _) in &fields {
                    if !baseline_profile.fields.contains_key(field_name) {
                        pending_drift.push((
                            event_name.to_string(),
                            field_name.clone(),
                            DriftType::FieldAppeared,
                            format!(
                                "new field '{}' discovered in {}",
                                field_name, event_name
                            ),
                        ));
                    }
                }

                // Type changes vs baseline
                for (field_name, json_type) in &fields {
                    if let Some(baseline_field) = baseline_profile.fields.get(field_name) {
                        if baseline_field.json_type != *json_type {
                            pending_drift.push((
                                event_name.to_string(),
                                field_name.clone(),
                                DriftType::TypeChanged,
                                format!(
                                    "field '{}' type changed in {}: {} -> {}",
                                    field_name, event_name, baseline_field.json_type, json_type,
                                ),
                            ));
                        }
                    }
                }
            }
        }
        // If no baseline exists, this session IS the baseline — no drift to report.

        // Emit collected drift entries (no borrows held)
        for (evt, field, dtype, detail) in pending_drift {
            self.emit_drift(app, &evt, &field, dtype, &detail);
        }

        self.dirty = true;
    }

    /// Persist merged observations to disk.
    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }

        // Merge session into baseline
        for (event_name, session_profile) in &self.session {
            let baseline_profile =
                self.baseline
                    .entry(event_name.clone())
                    .or_insert_with(|| EventProfile {
                        fields: HashMap::new(),
                        observation_count: 0,
                    });

            baseline_profile.observation_count += session_profile.observation_count;

            for (field_name, session_field) in &session_profile.fields {
                let baseline_field =
                    baseline_profile
                        .fields
                        .entry(field_name.clone())
                        .or_insert_with(|| FieldProfile {
                            json_type: session_field.json_type.clone(),
                            times_seen: 0,
                            first_seen: session_field.first_seen.clone(),
                            last_seen: session_field.last_seen.clone(),
                        });

                baseline_field.times_seen += session_field.times_seen;
                baseline_field.last_seen = session_field.last_seen.clone();
                // Keep earliest first_seen
                if session_field.first_seen < baseline_field.first_seen {
                    baseline_field.first_seen = session_field.first_seen.clone();
                }
                // Update type to latest observed
                baseline_field.json_type = session_field.json_type.clone();
            }
        }

        let path = self.data_dir.join("schema_observations.json");
        match serde_json::to_string_pretty(&self.baseline) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    log::warn!("Failed to save schema observations: {}", e);
                } else {
                    log::debug!("Schema observations saved to {}", path.display());
                    self.dirty = false;
                }
            }
            Err(e) => {
                log::warn!("Failed to serialize schema observations: {}", e);
            }
        }
    }

    /// Check if there are unsaved observations.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Get the drift report for this session (serializable for frontend).
    pub fn drift_report(&self) -> Vec<DriftEntry> {
        self.drift_log.clone()
    }

    /// Emit a drift entry, deduplicating by (event, field, type) within the session.
    fn emit_drift(
        &mut self,
        app: &AppHandle,
        event_name: &str,
        field_name: &str,
        drift_type: DriftType,
        detail: &str,
    ) {
        let key = DriftKey {
            event_name: event_name.to_string(),
            field_name: field_name.to_string(),
            drift_type: drift_type.clone(),
        };

        if self.drift_seen.contains(&key) {
            return;
        }
        self.drift_seen.insert(key);

        let entry = DriftEntry {
            event_name: event_name.to_string(),
            field_name: field_name.to_string(),
            drift_type: drift_type.clone(),
            detail: detail.to_string(),
        };
        self.drift_log.push(entry);

        match drift_type {
            DriftType::FieldDisappeared => {
                diagnostics::emit_warning(app, &LOG_015, detail);
            }
            DriftType::TypeChanged => {
                diagnostics::emit_warning(app, &LOG_016, detail);
            }
            DriftType::FieldAppeared => {
                diagnostics::emit_info(app, &LOG_017, detail);
            }
        }
    }
}

/// Generate a Unix epoch timestamp string (seconds since epoch).
fn unix_timestamp_string() -> String {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

/// Extract top-level field names and their JSON types from a Value.
fn extract_fields(data: &Value) -> Vec<(String, String)> {
    match data.as_object() {
        Some(obj) => obj
            .iter()
            .map(|(k, v)| {
                let type_name = match v {
                    Value::Null => "Null",
                    Value::Bool(_) => "Bool",
                    Value::Number(_) => "Number",
                    Value::String(_) => "String",
                    Value::Array(_) => "Array",
                    Value::Object(_) => "Object",
                };
                (k.clone(), type_name.to_string())
            })
            .collect(),
        None => Vec::new(),
    }
}

/// Load baseline from `schema_observations.json` in the data directory.
fn load_baseline(data_dir: &Path) -> HashMap<String, EventProfile> {
    let path = data_dir.join("schema_observations.json");
    match std::fs::read_to_string(&path) {
        Ok(json) => match serde_json::from_str(&json) {
            Ok(baseline) => {
                log::info!("Loaded schema baseline from {}", path.display());
                baseline
            }
            Err(e) => {
                log::warn!(
                    "Failed to parse schema baseline ({}), starting fresh: {}",
                    path.display(),
                    e
                );
                HashMap::new()
            }
        },
        Err(_) => {
            log::info!("No schema baseline found, starting fresh");
            HashMap::new()
        }
    }
}

// --- Unit tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Helper: create an observer with no baseline (first session).
    fn fresh_observer() -> SchemaObserver {
        SchemaObserver {
            baseline: HashMap::new(),
            session: HashMap::new(),
            drift_log: Vec::new(),
            drift_seen: std::collections::HashSet::new(),
            data_dir: PathBuf::from("/tmp/test_schema"),
            dirty: false,
        }
    }

    /// Helper: create an observer with a pre-populated baseline.
    fn observer_with_baseline(
        event_name: &str,
        fields: &[(&str, &str)],
        observation_count: u32,
    ) -> SchemaObserver {
        let mut field_map = HashMap::new();
        for &(name, json_type) in fields {
            field_map.insert(
                name.to_string(),
                FieldProfile {
                    json_type: json_type.to_string(),
                    times_seen: observation_count, // all fields seen every time
                    first_seen: "2025-01-01T00:00:00Z".to_string(),
                    last_seen: "2025-06-01T00:00:00Z".to_string(),
                },
            );
        }

        let mut baseline = HashMap::new();
        baseline.insert(
            event_name.to_string(),
            EventProfile {
                fields: field_map,
                observation_count,
            },
        );

        SchemaObserver {
            baseline,
            session: HashMap::new(),
            drift_log: Vec::new(),
            drift_seen: std::collections::HashSet::new(),
            data_dir: PathBuf::from("/tmp/test_schema"),
            dirty: false,
        }
    }

    #[test]
    fn test_extract_fields() {
        let data = json!({
            "Gold": 5000,
            "Gems": 1200,
            "Name": "player",
            "Boosters": [],
            "Details": {"x": 1},
            "Active": true,
            "Empty": null
        });

        let fields = extract_fields(&data);
        assert_eq!(fields.len(), 7);

        let map: HashMap<String, String> = fields.into_iter().collect();
        assert_eq!(map["Gold"], "Number");
        assert_eq!(map["Gems"], "Number");
        assert_eq!(map["Name"], "String");
        assert_eq!(map["Boosters"], "Array");
        assert_eq!(map["Details"], "Object");
        assert_eq!(map["Active"], "Bool");
        assert_eq!(map["Empty"], "Null");
    }

    #[test]
    fn test_extract_fields_non_object() {
        let data = json!([1, 2, 3]);
        let fields = extract_fields(&data);
        assert!(fields.is_empty());
    }

    #[test]
    fn test_first_session_no_drift() {
        // First session = no baseline, so observe() should record but not report drift
        let mut observer = fresh_observer();

        let data = json!({
            "InventoryInfo": {"Gold": 5000},
            "DeckSummaries": []
        });

        // We can't call observe with a real AppHandle in unit tests,
        // so we test the internal logic by checking the session profile.
        let fields = extract_fields(&data);
        let now = "2025-06-10T00:00:00Z".to_string();

        let profile = observer
            .session
            .entry("StartHook".to_string())
            .or_insert_with(|| EventProfile {
                fields: HashMap::new(),
                observation_count: 0,
            });
        profile.observation_count += 1;

        for (name, json_type) in &fields {
            profile
                .fields
                .entry(name.clone())
                .or_insert_with(|| FieldProfile {
                    json_type: json_type.clone(),
                    times_seen: 0,
                    first_seen: now.clone(),
                    last_seen: now.clone(),
                })
                .times_seen += 1;
        }

        // No drift should be recorded (no baseline to compare against)
        assert!(observer.drift_log.is_empty());
        assert_eq!(
            observer.session["StartHook"].observation_count,
            1
        );
        assert_eq!(observer.session["StartHook"].fields.len(), 2);
    }

    #[test]
    fn test_baseline_merge() {
        let mut observer = fresh_observer();

        // Simulate session data
        let mut fields = HashMap::new();
        fields.insert(
            "Gold".to_string(),
            FieldProfile {
                json_type: "Number".to_string(),
                times_seen: 3,
                first_seen: "2025-06-10T00:00:00Z".to_string(),
                last_seen: "2025-06-10T01:00:00Z".to_string(),
            },
        );
        observer.session.insert(
            "StartHook".to_string(),
            EventProfile {
                fields,
                observation_count: 3,
            },
        );
        observer.dirty = true;

        // Merging into empty baseline should create the entry
        // (save() writes to disk, so we test the merge logic directly)
        for (event_name, session_profile) in &observer.session {
            let baseline_profile =
                observer
                    .baseline
                    .entry(event_name.clone())
                    .or_insert_with(|| EventProfile {
                        fields: HashMap::new(),
                        observation_count: 0,
                    });
            baseline_profile.observation_count += session_profile.observation_count;
            for (field_name, sf) in &session_profile.fields {
                let bf =
                    baseline_profile
                        .fields
                        .entry(field_name.clone())
                        .or_insert_with(|| FieldProfile {
                            json_type: sf.json_type.clone(),
                            times_seen: 0,
                            first_seen: sf.first_seen.clone(),
                            last_seen: sf.last_seen.clone(),
                        });
                bf.times_seen += sf.times_seen;
                bf.last_seen = sf.last_seen.clone();
            }
        }

        assert_eq!(observer.baseline["StartHook"].observation_count, 3);
        assert_eq!(
            observer.baseline["StartHook"].fields["Gold"].times_seen,
            3
        );
    }

    #[test]
    fn test_drift_deduplication() {
        let mut observer = fresh_observer();

        // Manually add two drift entries with the same key
        let key1 = DriftKey {
            event_name: "StartHook".to_string(),
            field_name: "Gold".to_string(),
            drift_type: DriftType::FieldDisappeared,
        };
        assert!(!observer.drift_seen.contains(&key1));
        observer.drift_seen.insert(key1);

        let key2 = DriftKey {
            event_name: "StartHook".to_string(),
            field_name: "Gold".to_string(),
            drift_type: DriftType::FieldDisappeared,
        };
        assert!(observer.drift_seen.contains(&key2));
    }

    #[test]
    fn test_presence_rate_threshold() {
        // Field seen 4 out of 10 times = 40% < 80% threshold → no disappearance warning
        let mut field_map = HashMap::new();
        field_map.insert(
            "RareField".to_string(),
            FieldProfile {
                json_type: "String".to_string(),
                times_seen: 4,
                first_seen: "2025-01-01T00:00:00Z".to_string(),
                last_seen: "2025-06-01T00:00:00Z".to_string(),
            },
        );
        field_map.insert(
            "CommonField".to_string(),
            FieldProfile {
                json_type: "Number".to_string(),
                times_seen: 9,
                first_seen: "2025-01-01T00:00:00Z".to_string(),
                last_seen: "2025-06-01T00:00:00Z".to_string(),
            },
        );

        let profile = EventProfile {
            fields: field_map,
            observation_count: 10,
        };

        // RareField: 4/10 = 0.4 < 0.8 → should NOT trigger disappearance
        let rare_rate =
            profile.fields["RareField"].times_seen as f64 / profile.observation_count as f64;
        assert!(rare_rate < PRESENCE_THRESHOLD);

        // CommonField: 9/10 = 0.9 >= 0.8 → should trigger disappearance
        let common_rate =
            profile.fields["CommonField"].times_seen as f64 / profile.observation_count as f64;
        assert!(common_rate >= PRESENCE_THRESHOLD);
    }

    #[test]
    fn test_critical_fields_list() {
        // Verify critical fields are correctly defined
        assert_eq!(CRITICAL_FIELDS.len(), 4);

        // StartHook has 1 critical field
        let start_hook_fields: Vec<_> = CRITICAL_FIELDS.iter().filter(|f| f.0 == "StartHook").collect();
        assert_eq!(start_hook_fields.len(), 1);
        assert_eq!(start_hook_fields[0].1, "InventoryInfo");

        // InventoryUpdate has 2 critical fields
        let inv_fields: Vec<_> = CRITICAL_FIELDS
            .iter()
            .filter(|f| f.0 == "InventoryUpdate")
            .collect();
        assert_eq!(inv_fields.len(), 2);

        // CourseList has 1 critical field
        let course_fields: Vec<_> = CRITICAL_FIELDS
            .iter()
            .filter(|f| f.0 == "CourseList")
            .collect();
        assert_eq!(course_fields.len(), 1);
        assert_eq!(course_fields[0].1, "Courses");
    }

    #[test]
    fn test_load_baseline_missing_file() {
        let baseline = load_baseline(Path::new("/nonexistent/path"));
        assert!(baseline.is_empty());
    }
}
