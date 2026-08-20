// Event classification for Player.log JSON lines.
//
// Port of Python's `_EVENT_NAMES` + `_classify_event()` from `log_watcher.py`.
// Divergence: returns typed enum instead of string.

use serde_json::Value;

/// Classified event types from Player.log JSON blobs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogEventType {
    StartHook,
    InventoryUpdate,
    CourseList,
    QuestUpdate,
    RankProgress,
    RankInfo,
    MatchHistory,
    MatchRoomState,
    DeckUpdate,
    EventProgress,
    FormatList,
    PreconDecks,
    DeckSummaries,
    ApiResponse,
    GameEvent,
    ModulePayload,
    ProgressUpdate,
    RewardChestConfig,
    Unknown,
}

impl LogEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::StartHook => "StartHook",
            Self::InventoryUpdate => "InventoryUpdate",
            Self::CourseList => "CourseList",
            Self::QuestUpdate => "QuestUpdate",
            Self::RankProgress => "RankProgress",
            Self::RankInfo => "RankInfo",
            Self::MatchHistory => "MatchHistory",
            Self::MatchRoomState => "MatchRoomState",
            Self::DeckUpdate => "DeckUpdate",
            Self::EventProgress => "EventProgress",
            Self::FormatList => "FormatList",
            Self::PreconDecks => "PreconDecks",
            Self::DeckSummaries => "DeckSummaries",
            Self::ApiResponse => "APIResponse",
            Self::GameEvent => "GameEvent",
            Self::ModulePayload => "ModulePayload",
            Self::ProgressUpdate => "ProgressUpdate",
            Self::RewardChestConfig => "RewardChestConfig",
            Self::Unknown => "Unknown",
        }
    }
}

/// Signature entry: set of required keys → event type.
struct Signature {
    keys: &'static [&'static str],
    event_type: LogEventType,
}

/// Event signatures ordered from most specific (most keys) to least specific.
/// StartHook must come before InventoryUpdate since InventoryUpdate's keys
/// are a subset of StartHook's.
const SIGNATURES: &[Signature] = &[
    Signature {
        keys: &["InventoryInfo", "DeckSummaries", "Decks"],
        event_type: LogEventType::StartHook,
    },
    Signature {
        keys: &["Course", "InventoryInfo"],
        event_type: LogEventType::InventoryUpdate,
    },
    // Push notification InventoryChanged — arrives without Course key.
    // Covers wildcard crafting, pack opens, quest rewards, etc.
    // Must come after StartHook (which also has InventoryInfo).
    Signature {
        keys: &["InventoryInfo"],
        event_type: LogEventType::InventoryUpdate,
    },
    // MatchGameRoomStateChanged — must come BEFORE ApiResponse because the
    // event also has {transactionId, requestId, timestamp} which would match
    // the less-specific ApiResponse signature first.
    Signature {
        keys: &["matchGameRoomStateChangedEvent"],
        event_type: LogEventType::MatchRoomState,
    },
    Signature {
        keys: &["transactionId", "requestId", "timestamp"],
        event_type: LogEventType::ApiResponse,
    },
    Signature {
        keys: &["timestamp", "greToClientEvent"],
        event_type: LogEventType::GameEvent,
    },
    Signature {
        keys: &["Courses"],
        event_type: LogEventType::CourseList,
    },
    Signature {
        keys: &["NodeStates", "MilestoneStates"],
        event_type: LogEventType::ProgressUpdate,
    },
    Signature {
        keys: &["canSwap", "quests"],
        event_type: LogEventType::QuestUpdate,
    },
    // RankProgress: compact rank update with flat keys. Must come before RankInfo
    // since RankInfo uses nested object keys that don't overlap.
    Signature {
        keys: &[
            "constructedSeasonOrdinal",
            "constructedLevel",
            "constructedStep",
            "constructedMatchesWon",
            "limitedSeasonOrdinal",
            "limitedLevel",
        ],
        event_type: LogEventType::RankProgress,
    },
    Signature {
        keys: &["currentSeason", "limitedRankInfo", "constructedRankInfo"],
        event_type: LogEventType::RankInfo,
    },
    Signature {
        keys: &["DeckId", "Name", "Attributes"],
        event_type: LogEventType::DeckUpdate,
    },
    Signature {
        keys: &["CourseId", "InternalEventName", "CurrentModule"],
        event_type: LogEventType::EventProgress,
    },
    Signature {
        keys: &["MatchesV3"],
        event_type: LogEventType::MatchHistory,
    },
    Signature {
        keys: &["CurrentModule", "Payload"],
        event_type: LogEventType::ModulePayload,
    },
    Signature {
        keys: &["Formats", "FormatGroups"],
        event_type: LogEventType::FormatList,
    },
    Signature {
        keys: &["CacheVersion", "PreconDecks"],
        event_type: LogEventType::PreconDecks,
    },
    Signature {
        keys: &["Summaries"],
        event_type: LogEventType::DeckSummaries,
    },
    Signature {
        keys: &[
            "_dailyRewardChestDescriptions",
            "_weeklyRewardChestDescriptions",
        ],
        event_type: LogEventType::RewardChestConfig,
    },
];

/// Classify a parsed JSON object by its top-level key signature.
///
/// Checks each signature's required keys against the object. Returns the first
/// match (signatures are ordered most-specific first). Falls back to `Unknown`.
pub fn classify(json: &Value) -> LogEventType {
    let obj = match json.as_object() {
        Some(o) => o,
        None => return LogEventType::Unknown,
    };

    for sig in SIGNATURES {
        if sig.keys.iter().all(|k| obj.contains_key(*k)) {
            return sig.event_type.clone();
        }
    }

    LogEventType::Unknown
}

/// Map a rank level integer to its class name.
fn rank_class_name(level: i64) -> &'static str {
    match level {
        0 => "Beginner",
        1 => "Bronze",
        2 => "Silver",
        3 => "Gold",
        4 => "Platinum",
        5 => "Diamond",
        6 => "Mythic",
        _ => "Unknown",
    }
}

/// Format a rank as "Class Tier N" (e.g. "Gold Tier 2").
fn format_rank(class_name: &str, tier: i64) -> String {
    if class_name == "Mythic" {
        "Mythic".to_string()
    } else {
        format!("{} Tier {}", class_name, tier)
    }
}

/// Search multiple locations for Wins/Losses in an EventProgress payload.
/// Returns (Some(wins), Some(losses)) if found, (None, None) if absent.
/// Checked locations: root object, CourseDeckSummary, ModulePayload.
pub fn find_win_loss(json: &Value) -> (Option<i64>, Option<i64>) {
    // Check root
    if let Some(w) = json.get("Wins").and_then(|v| v.as_i64()) {
        let l = json.get("Losses").and_then(|v| v.as_i64());
        return (Some(w), l);
    }
    // Check CourseDeckSummary
    if let Some(deck) = json.get("CourseDeckSummary") {
        if let Some(w) = deck.get("Wins").and_then(|v| v.as_i64()) {
            let l = deck.get("Losses").and_then(|v| v.as_i64());
            return (Some(w), l);
        }
    }
    // Check ModulePayload
    if let Some(payload) = json.get("ModulePayload") {
        if let Some(w) = payload.get("Wins").and_then(|v| v.as_i64()) {
            let l = payload.get("Losses").and_then(|v| v.as_i64());
            return (Some(w), l);
        }
    }
    (None, None)
}

/// Find MaxWins/MaxLosses in the same locations.
pub fn find_max_win_loss(json: &Value) -> (Option<i64>, Option<i64>) {
    for source in &[
        json,
        json.get("CourseDeckSummary").unwrap_or(&Value::Null),
        json.get("ModulePayload").unwrap_or(&Value::Null),
    ] {
        if let Some(mw) = source.get("MaxWins").and_then(|v| v.as_i64()) {
            let ml = source.get("MaxLosses").and_then(|v| v.as_i64());
            return (Some(mw), ml);
        }
    }
    (None, None)
}

/// Try to produce a short human-readable summary for a classified event.
/// Uses the JSON payload directly to build contextual summaries.
pub fn summarize(event_type: &LogEventType, json: &Value) -> String {
    match event_type {
        LogEventType::StartHook => {
            let deck_count = json
                .get("DeckSummaries")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            format!("Session start — {} decks", deck_count)
        }
        LogEventType::InventoryUpdate => {
            let changes = json
                .get("InventoryInfo")
                .and_then(|v| v.get("Changes"))
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            format!("{} inventory change(s)", changes)
        }
        LogEventType::QuestUpdate => {
            // Try to show the first quest with active progress
            if let Some(quests) = json.get("quests").and_then(|v| v.as_array()) {
                for q in quests {
                    let max = q.get("progressMax").and_then(|v| v.as_i64()).unwrap_or(0);
                    if max <= 0 {
                        continue;
                    }
                    let title = q.get("title").and_then(|v| v.as_str()).unwrap_or("");
                    let progress = q.get("progress").and_then(|v| v.as_i64()).unwrap_or(0);
                    if !title.is_empty() {
                        return format!("{} ({}/{})", title, progress, max);
                    }
                }
                format!("{} quest(s)", quests.len())
            } else {
                "Quests updated".to_string()
            }
        }
        LogEventType::RankInfo => {
            let mut parts = Vec::new();
            for (key, label) in &[
                ("constructedRankInfo", "Constructed"),
                ("limitedRankInfo", "Limited"),
            ] {
                if let Some(info) = json.get(*key) {
                    let level = info.get("level").and_then(|v| v.as_i64()).unwrap_or(-1);
                    let step = info.get("step").and_then(|v| v.as_i64()).unwrap_or(0);
                    if level >= 0 {
                        let class = rank_class_name(level);
                        parts.push(format!("{} ({})", format_rank(class, step), label));
                    }
                }
            }
            if parts.is_empty() {
                "Rank update".to_string()
            } else {
                parts.join(", ")
            }
        }
        LogEventType::RankProgress => {
            let mut parts = Vec::new();
            let c_level = json.get("constructedLevel").and_then(|v| v.as_i64());
            let c_step = json.get("constructedStep").and_then(|v| v.as_i64());
            if let (Some(level), Some(step)) = (c_level, c_step) {
                let class = rank_class_name(level);
                parts.push(format!("{} (Constructed)", format_rank(class, step)));
            }
            // Limited: only level is documented in compact format (no step)
            let l_level = json.get("limitedLevel").and_then(|v| v.as_i64());
            let l_step = json.get("limitedStep").and_then(|v| v.as_i64());
            match (l_level, l_step) {
                (Some(level), Some(step)) => {
                    let class = rank_class_name(level);
                    parts.push(format!("{} (Limited)", format_rank(class, step)));
                }
                (Some(level), None) => {
                    parts.push(format!("{} (Limited)", rank_class_name(level)));
                }
                _ => {}
            }
            if parts.is_empty() {
                "Rank progress".to_string()
            } else {
                parts.join(", ")
            }
        }
        LogEventType::DeckUpdate => {
            json.get("Name")
                .and_then(|v| v.as_str())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "Deck updated".to_string())
        }
        LogEventType::EventProgress => {
            let event_name = json
                .get("InternalEventName")
                .and_then(|v| v.as_str())
                .unwrap_or("Event");
            // Win/loss location is unverified — check root, CourseDeckSummary,
            // and ModulePayload. Only show record if keys actually exist.
            let (wins, losses) = find_win_loss(json);
            if wins.is_some() || losses.is_some() {
                format!("{} ({}-{})", event_name, wins.unwrap_or(0), losses.unwrap_or(0))
            } else {
                let module = json.get("CurrentModule")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if module.is_empty() {
                    event_name.to_string()
                } else {
                    format!("{} — {}", event_name, module)
                }
            }
        }
        LogEventType::MatchRoomState => {
            if let Some(state_type) = json
                .pointer("/matchGameRoomStateChangedEvent/gameRoomInfo/stateType")
                .and_then(|v| v.as_str())
            {
                let label = state_type
                    .strip_prefix("MatchGameRoomStateType_")
                    .unwrap_or(state_type);
                format!("Match room — {}", label)
            } else {
                "Match room state changed".to_string()
            }
        }
        LogEventType::MatchHistory => "Match history".to_string(),
        LogEventType::GameEvent => "Game event".to_string(),
        _ => event_type.as_str().to_string(),
    }
}

/// Extract structured detail fields from a classified event's JSON payload.
/// Returns `None` for event types that don't have enriched details.
pub fn extract_details(event_type: &LogEventType, json: &Value) -> Option<Value> {
    match event_type {
        LogEventType::QuestUpdate => extract_quest_details(json),
        LogEventType::RankInfo => extract_rank_info_details(json),
        LogEventType::RankProgress => extract_rank_progress_details(json),
        LogEventType::DeckUpdate => extract_deck_details(json),
        LogEventType::EventProgress => extract_event_progress_details(json),
        _ => None,
    }
}

fn extract_quest_details(json: &Value) -> Option<Value> {
    let quests_arr = json.get("quests")?.as_array()?;
    let can_swap = json.get("canSwap").and_then(|v| v.as_bool()).unwrap_or(false);

    let mut quests = Vec::new();
    for q in quests_arr {
        let max = q.get("progressMax").and_then(|v| v.as_i64()).unwrap_or(0);
        if max <= 0 {
            continue; // Skip completed/empty quests
        }
        let title = q.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let progress = q.get("progress").and_then(|v| v.as_i64()).unwrap_or(0);
        quests.push(serde_json::json!({
            "title": title,
            "progress": progress,
            "goal": max,
        }));
    }

    if quests.is_empty() {
        return None;
    }

    Some(serde_json::json!({
        "quests": quests,
        "can_swap": can_swap,
    }))
}

fn extract_rank_detail(info: &Value) -> Option<Value> {
    let level = info.get("level").and_then(|v| v.as_i64())?;
    let step = info.get("step").and_then(|v| v.as_i64()).unwrap_or(0);
    let won = info.get("matchesWon").and_then(|v| v.as_i64()).unwrap_or(0);
    let lost = info.get("matchesLost").and_then(|v| v.as_i64()).unwrap_or(0);
    let class = rank_class_name(level);

    Some(serde_json::json!({
        "class": class,
        "tier": step,
        "step": step,
        "won": won,
        "lost": lost,
    }))
}

fn extract_rank_info_details(json: &Value) -> Option<Value> {
    let constructed = json
        .get("constructedRankInfo")
        .and_then(extract_rank_detail);
    let limited = json.get("limitedRankInfo").and_then(extract_rank_detail);

    if constructed.is_none() && limited.is_none() {
        return None;
    }

    Some(serde_json::json!({
        "constructed": constructed,
        "limited": limited,
    }))
}

fn extract_rank_progress_details(json: &Value) -> Option<Value> {
    // Compact RankProgress only has constructedLevel/Step/MatchesWon and
    // limitedSeasonOrdinal/limitedLevel — no limitedStep or limitedMatchesWon.
    // Only emit a rank section when we have both level AND step (proof the
    // data is actually present, not defaulted to 0).
    let c_level = json.get("constructedLevel").and_then(|v| v.as_i64());
    let c_step = json.get("constructedStep").and_then(|v| v.as_i64());
    let c_won = json.get("constructedMatchesWon").and_then(|v| v.as_i64());

    let constructed = match (c_level, c_step) {
        (Some(level), Some(step)) => Some(serde_json::json!({
            "class": rank_class_name(level),
            "tier": step,
            "step": step,
            "won": c_won.unwrap_or(0),
        })),
        _ => None,
    };

    // Limited: only level is documented (no step/matchesWon in compact format).
    // Emit class only when level exists, but don't fabricate tier/step.
    let l_level = json.get("limitedLevel").and_then(|v| v.as_i64());
    let l_step = json.get("limitedStep").and_then(|v| v.as_i64());
    let l_won = json.get("limitedMatchesWon").and_then(|v| v.as_i64());

    let limited = match (l_level, l_step) {
        // If both exist (unexpected but possible in future patches), use them
        (Some(level), Some(step)) => Some(serde_json::json!({
            "class": rank_class_name(level),
            "tier": step,
            "step": step,
            "won": l_won.unwrap_or(0),
        })),
        // Level only — show class without tier detail
        (Some(level), None) => Some(serde_json::json!({
            "class": rank_class_name(level),
        })),
        _ => None,
    };

    if constructed.is_none() && limited.is_none() {
        return None;
    }

    Some(serde_json::json!({
        "constructed": constructed,
        "limited": limited,
    }))
}

fn extract_deck_details(json: &Value) -> Option<Value> {
    let name = json.get("Name").and_then(|v| v.as_str())?;
    let format = json
        .get("Attributes")
        .and_then(|v| v.get("Format"))
        .and_then(|v| v.as_str());

    let mut detail = serde_json::json!({ "name": name });
    if let Some(fmt) = format {
        detail["format"] = serde_json::json!(fmt);
    }
    Some(detail)
}

fn extract_event_progress_details(json: &Value) -> Option<Value> {
    let event_name = json
        .get("InternalEventName")
        .and_then(|v| v.as_str())?
        .to_string();
    let module = json
        .get("CurrentModule")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Win/loss location is unverified — search root, CourseDeckSummary, ModulePayload
    let (wins, losses) = find_win_loss(json);
    let (max_wins, max_losses) = find_max_win_loss(json);

    let mut detail = serde_json::json!({
        "event_name": event_name,
        "module": module,
    });

    // Only include win/loss fields when data actually exists in the payload
    if wins.is_some() || losses.is_some() {
        detail["wins"] = serde_json::json!(wins.unwrap_or(0));
        detail["losses"] = serde_json::json!(losses.unwrap_or(0));
    }
    if max_wins.is_some() || max_losses.is_some() {
        detail["max_wins"] = serde_json::json!(max_wins.unwrap_or(0));
        detail["max_losses"] = serde_json::json!(max_losses.unwrap_or(0));
    }

    Some(detail)
}
