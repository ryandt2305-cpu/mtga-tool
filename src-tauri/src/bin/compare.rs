// Standalone comparison binary — exports card DB and collection as JSON
// for diffing against Python reference output. No Tauri dependency.
//
// Usage:
//   cargo run --bin compare -- card-db [--path <override>] [--output <dir>]
//   cargo run --bin compare -- collection [--path <override>] [--output <dir>]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use mtga_hub_lib::models::{CardInfo, Rarity};
use mtga_hub_lib::services::card_db::{self, LoadResult};

// ── JSON serialization helpers ──────────────────────────────────────

/// Serialize Rarity as a flat string matching Python's RARITY_MAP format.
/// Known values → snake_case string, unknown → "unknown_N".
fn rarity_to_string(rarity: &Rarity) -> String {
    match rarity {
        Rarity::Special => "special".to_string(),
        Rarity::BasicLand => "basic_land".to_string(),
        Rarity::Common => "common".to_string(),
        Rarity::Uncommon => "uncommon".to_string(),
        Rarity::Rare => "rare".to_string(),
        Rarity::Mythic => "mythic".to_string(),
        Rarity::Unknown(n) => format!("unknown_{}", n),
    }
}

/// Build a JSON array entry for a card (card-db mode).
fn card_to_json(info: &CardInfo) -> serde_json::Value {
    serde_json::json!({
        "id": info.grp_id,
        "name": info.name,
        "set": info.set_code,
        "rarity": rarity_to_string(&info.rarity),
        "collector_number": info.collector_number,
        "deck_limit": info.deck_limit,
    })
}

/// Build a JSON array entry for a collected card (collection mode).
fn collection_entry_to_json(info: &CardInfo, quantity: i32) -> serde_json::Value {
    serde_json::json!({
        "id": info.grp_id,
        "name": info.name,
        "set": info.set_code,
        "rarity": rarity_to_string(&info.rarity),
        "quantity": quantity,
        "collector_number": info.collector_number,
    })
}

// ── Argument parsing ────────────────────────────────────────────────

struct Args {
    command: String,
    path_override: Option<String>,
    output_dir: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        return Err(format!(
            "Usage: {} <card-db|collection> [--path <override>] [--output <dir>]",
            args[0]
        ));
    }

    let command = args[1].clone();
    if command != "card-db" && command != "collection" {
        return Err(format!("Unknown command: {}. Use 'card-db' or 'collection'.", command));
    }

    let mut path_override = None;
    let mut output_dir = PathBuf::from("tests/compare/output");
    let mut i = 2;

    while i < args.len() {
        match args[i].as_str() {
            "--path" => {
                i += 1;
                path_override = Some(
                    args.get(i)
                        .ok_or("--path requires a value")?
                        .clone(),
                );
            }
            "--output" => {
                i += 1;
                output_dir = PathBuf::from(
                    args.get(i)
                        .ok_or("--output requires a value")?,
                );
            }
            other => return Err(format!("Unknown flag: {}", other)),
        }
        i += 1;
    }

    Ok(Args {
        command,
        path_override,
        output_dir,
    })
}

// ── Subcommands ─────────────────────────────────────────────────────

fn cmd_card_db(args: &Args) -> Result<(), String> {
    eprintln!("Resolving card database path...");
    let db_path = card_db::resolve_db_path(args.path_override.as_deref())
        .map_err(|e| e.to_string())?;

    eprintln!("Loading from: {}", db_path.display());
    let result: LoadResult = card_db::load_cards_from_path(&db_path)
        .map_err(|e| e.to_string())?;

    eprintln!("Loaded {} cards", result.cards.len());

    // Sort by id for deterministic output
    let mut entries: Vec<&CardInfo> = result.cards.values().collect();
    entries.sort_by_key(|c| c.grp_id);

    let json_array: Vec<serde_json::Value> = entries.iter().map(|c| card_to_json(c)).collect();

    // Write output
    std::fs::create_dir_all(&args.output_dir)
        .map_err(|e| format!("Failed to create output dir: {}", e))?;

    let out_path = args.output_dir.join("rust_cards.json");
    let json_str = serde_json::to_string_pretty(&json_array)
        .map_err(|e| format!("JSON serialization failed: {}", e))?;

    std::fs::write(&out_path, &json_str)
        .map_err(|e| format!("Failed to write {}: {}", out_path.display(), e))?;

    eprintln!("Written to {}", out_path.display());
    Ok(())
}

#[cfg(windows)]
fn cmd_collection(args: &Args) -> Result<(), String> {
    use mtga_hub_lib::services::memory::process;
    use mtga_hub_lib::services::memory::scanner;

    // Load card database first (need valid_ids)
    eprintln!("Loading card database...");
    let db_path = card_db::resolve_db_path(args.path_override.as_deref())
        .map_err(|e| e.to_string())?;
    let load_result = card_db::load_cards_from_path(&db_path)
        .map_err(|e| e.to_string())?;

    eprintln!("Loaded {} cards", load_result.cards.len());

    // Find MTGA.exe
    eprintln!("Looking for MTGA.exe...");
    let pid = process::find_process("MTGA.exe")
        .map_err(|e| format!("Process enumeration failed: {}", e))?
        .ok_or("MTGA.exe is not running")?;

    eprintln!("Found MTGA.exe (PID {})", pid);

    // Open process and enumerate regions
    let handle = process::ProcessHandle::open(pid)
        .map_err(|e| format!("Failed to open process: {}", e))?;

    let regions = handle.enumerate_regions()
        .map_err(|e| format!("Failed to enumerate regions: {}", e))?;

    eprintln!("{} readable memory regions", regions.len());

    // Scan
    let empty_anchors: HashSet<i32> = HashSet::new();
    let candidates = scanner::scan_all_regions(
        &handle,
        &regions,
        &load_result.valid_ids,
        &empty_anchors,
        Some(&|current, total| {
            if current % 100 == 0 || current == total {
                eprint!("\rScanning region {}/{}...", current, total);
            }
        }),
        None,
        0,
    );
    eprintln!();

    if candidates.is_empty() {
        return Err("No collection data found in memory".to_string());
    }

    eprintln!(
        "Found {} candidates. Best: {} entries, score={:.0}",
        candidates.len(),
        candidates[0].entries.len(),
        candidates[0].score(),
    );

    let non_collectible: HashSet<i32> = HashSet::new();
    let collection: HashMap<i32, i32> = scanner::build_collection(&candidates[0], &non_collectible);

    // Build JSON output sorted by id
    let mut ids: Vec<i32> = collection.keys().copied().collect();
    ids.sort();

    let json_array: Vec<serde_json::Value> = ids
        .iter()
        .map(|&id| {
            let quantity = collection[&id];
            match load_result.cards.get(&id) {
                Some(info) => collection_entry_to_json(info, quantity),
                None => serde_json::json!({
                    "id": id,
                    "name": format!("Unknown ({})", id),
                    "set": "",
                    "rarity": "",
                    "quantity": quantity,
                    "collector_number": "",
                }),
            }
        })
        .collect();

    // Write output
    std::fs::create_dir_all(&args.output_dir)
        .map_err(|e| format!("Failed to create output dir: {}", e))?;

    let out_path = args.output_dir.join("rust_collection.json");
    let json_str = serde_json::to_string_pretty(&json_array)
        .map_err(|e| format!("JSON serialization failed: {}", e))?;

    std::fs::write(&out_path, &json_str)
        .map_err(|e| format!("Failed to write {}: {}", out_path.display(), e))?;

    eprintln!(
        "Written {} cards to {}",
        collection.len(),
        out_path.display()
    );
    Ok(())
}

#[cfg(not(windows))]
fn cmd_collection(_args: &Args) -> Result<(), String> {
    Err("Collection scanning requires Windows (MTGA.exe memory access)".to_string())
}

// ── Main ────────────────────────────────────────────────────────────

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let result = match args.command.as_str() {
        "card-db" => cmd_card_db(&args),
        "collection" => cmd_collection(&args),
        _ => unreachable!(),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
