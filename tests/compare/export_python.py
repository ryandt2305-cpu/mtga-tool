"""Export Python card DB and collection as JSON for comparison with Rust.

Usage:
    python tests/compare/export_python.py card-db [--path <override>] [--output <dir>]
    python tests/compare/export_python.py collection [--path <override>] [--output <dir>]

Run from the mtga-tool/ directory so Python can find the python.src package.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

# Add mtga-tool root to path so we can import python.src
sys.path.insert(0, str(Path(__file__).resolve().parent.parent.parent))

from python.src.card_database import (
    find_card_database,
    find_mtga_install,
    load_card_database,
)

DEFAULT_OUTPUT = Path(__file__).resolve().parent / "output"


def cmd_card_db(path_override: str | None, output_dir: Path) -> int:
    """Export card database as JSON."""
    if path_override:
        db_path = Path(path_override)
        if not db_path.exists():
            print(f"Error: path does not exist: {path_override}", file=sys.stderr)
            return 1
    else:
        install_dir = find_mtga_install()
        db_path = find_card_database(install_dir)

    print(f"Loading from: {db_path}", file=sys.stderr)
    cards, valid_ids = load_card_database(db_path)
    print(f"Loaded {len(cards)} cards", file=sys.stderr)

    # Sort by id, matching Rust output format
    entries = []
    for card_id in sorted(cards.keys()):
        info = cards[card_id]
        entries.append({
            "id": info.grp_id,
            "name": info.name,
            "set": info.set_code,
            "rarity": info.rarity,
            "collector_number": info.collector_number,
        })

    output_dir.mkdir(parents=True, exist_ok=True)
    out_path = output_dir / "python_cards.json"
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(entries, f, indent=2, ensure_ascii=False)

    print(f"Written to {out_path}", file=sys.stderr)
    return 0


def cmd_collection(path_override: str | None, output_dir: Path) -> int:
    """Export scanned collection as JSON."""
    from python.src.memory_scanner import scan_collection

    # Load card DB
    if path_override:
        db_path = Path(path_override)
    else:
        install_dir = find_mtga_install()
        db_path = find_card_database(install_dir)

    cards, valid_ids = load_card_database(db_path)
    print(f"Loaded {len(cards)} cards", file=sys.stderr)

    # Scan memory (no anchors/non-collectible for comparison — match Rust)
    print("Scanning MTGA memory...", file=sys.stderr)
    result = scan_collection(valid_ids=valid_ids)
    collection = result.collection
    print(f"Found {len(collection)} unique cards", file=sys.stderr)

    # Sort by id, matching Rust output format
    entries = []
    for card_id in sorted(collection.keys()):
        quantity = collection[card_id]
        info = cards.get(card_id)
        entries.append({
            "id": card_id,
            "name": info.name if info else f"Unknown ({card_id})",
            "set": info.set_code if info else "",
            "rarity": info.rarity if info else "",
            "quantity": quantity,
            "collector_number": info.collector_number if info else "",
        })

    output_dir.mkdir(parents=True, exist_ok=True)
    out_path = output_dir / "python_collection.json"
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(entries, f, indent=2, ensure_ascii=False)

    print(f"Written {len(collection)} cards to {out_path}", file=sys.stderr)
    return 0


def main() -> int:
    import argparse

    parser = argparse.ArgumentParser(description="Export Python data for comparison")
    parser.add_argument("command", choices=["card-db", "collection"])
    parser.add_argument("--path", help="Override card database path")
    parser.add_argument("--output", help="Output directory", default=str(DEFAULT_OUTPUT))
    args = parser.parse_args()

    output_dir = Path(args.output)

    if args.command == "card-db":
        return cmd_card_db(args.path, output_dir)
    elif args.command == "collection":
        return cmd_collection(args.path, output_dir)
    return 1


if __name__ == "__main__":
    sys.exit(main())
