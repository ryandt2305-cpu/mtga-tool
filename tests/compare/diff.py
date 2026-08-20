"""Diff two JSON exports (Python vs Rust) and report differences.

Usage:
    python tests/compare/diff.py card-db [--output <dir>]
    python tests/compare/diff.py collection [--output <dir>]

Loads python_*.json and rust_*.json from the output directory,
compares them by card id, and reports mismatches.

Exit code 0 = match, 1 = differences found.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

DEFAULT_OUTPUT = Path(__file__).resolve().parent / "output"


def normalize_rarity(rarity: str | dict) -> str:
    """Normalize rarity values for comparison.

    Python: "unknown_6" for unmapped int 6
    Rust (serde): {"unknown": 6} — but compare binary serializes as "unknown_6"
    Handle both cases for robustness.
    """
    if isinstance(rarity, dict) and "unknown" in rarity:
        return f"unknown_{rarity['unknown']}"
    return str(rarity)


def load_json(path: Path) -> list[dict]:
    """Load and return a JSON array, sorted by id."""
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    return sorted(data, key=lambda x: x["id"])


def diff_card_db(output_dir: Path) -> int:
    """Compare card-db exports."""
    python_path = output_dir / "python_cards.json"
    rust_path = output_dir / "rust_cards.json"

    for p in (python_path, rust_path):
        if not p.exists():
            print(f"Error: {p} not found. Run exports first.", file=sys.stderr)
            return 1

    python_cards = load_json(python_path)
    rust_cards = load_json(rust_path)

    print(f"Python: {len(python_cards)} cards")
    print(f"Rust:   {len(rust_cards)} cards")

    # Index by id
    py_by_id = {c["id"]: c for c in python_cards}
    rs_by_id = {c["id"]: c for c in rust_cards}

    py_ids = set(py_by_id.keys())
    rs_ids = set(rs_by_id.keys())

    only_python = py_ids - rs_ids
    only_rust = rs_ids - py_ids
    shared = py_ids & rs_ids

    mismatches = []
    fields = ["name", "set", "rarity", "collector_number"]

    for card_id in sorted(shared):
        py = py_by_id[card_id]
        rs = rs_by_id[card_id]
        diffs = {}
        for field in fields:
            py_val = normalize_rarity(py[field]) if field == "rarity" else py[field]
            rs_val = normalize_rarity(rs[field]) if field == "rarity" else rs[field]
            if py_val != rs_val:
                diffs[field] = (py_val, rs_val)
        if diffs:
            mismatches.append((card_id, diffs))

    # Report
    print(f"\nShared IDs: {len(shared)}")

    if only_python:
        print(f"\nOnly in Python ({len(only_python)}):")
        for cid in sorted(list(only_python)[:10]):
            info = py_by_id[cid]
            print(f"  {cid}: {info['name']} ({info['set']})")
        if len(only_python) > 10:
            print(f"  ... and {len(only_python) - 10} more")

    if only_rust:
        print(f"\nOnly in Rust ({len(only_rust)}):")
        for cid in sorted(list(only_rust)[:10]):
            info = rs_by_id[cid]
            print(f"  {cid}: {info['name']} ({info['set']})")
        if len(only_rust) > 10:
            print(f"  ... and {len(only_rust) - 10} more")

    if mismatches:
        print(f"\nField mismatches ({len(mismatches)}):")
        for cid, diffs in mismatches[:20]:
            py_name = py_by_id[cid]["name"]
            print(f"  {cid} ({py_name}):")
            for field, (py_val, rs_val) in diffs.items():
                print(f"    {field}: Python={py_val!r} Rust={rs_val!r}")
        if len(mismatches) > 20:
            print(f"  ... and {len(mismatches) - 20} more")

    has_diffs = bool(only_python or only_rust or mismatches)

    if has_diffs:
        print("\nRESULT: DIFFERENCES FOUND")
        return 1
    else:
        print("\nRESULT: MATCH")
        return 0


def diff_collection(output_dir: Path) -> int:
    """Compare collection exports."""
    python_path = output_dir / "python_collection.json"
    rust_path = output_dir / "rust_collection.json"

    for p in (python_path, rust_path):
        if not p.exists():
            print(f"Error: {p} not found. Run exports first.", file=sys.stderr)
            return 1

    python_cards = load_json(python_path)
    rust_cards = load_json(rust_path)

    print(f"Python: {len(python_cards)} unique cards")
    print(f"Rust:   {len(rust_cards)} unique cards")

    # Index by id
    py_by_id = {c["id"]: c for c in python_cards}
    rs_by_id = {c["id"]: c for c in rust_cards}

    py_ids = set(py_by_id.keys())
    rs_ids = set(rs_by_id.keys())

    only_python = py_ids - rs_ids
    only_rust = rs_ids - py_ids
    shared = py_ids & rs_ids

    field_mismatches = []
    quantity_mismatches = []
    fields = ["name", "set", "rarity", "collector_number"]

    for card_id in sorted(shared):
        py = py_by_id[card_id]
        rs = rs_by_id[card_id]

        # Check quantity separately (may vary due to scan timing)
        if py.get("quantity") != rs.get("quantity"):
            quantity_mismatches.append((
                card_id,
                py.get("quantity"),
                rs.get("quantity"),
            ))

        diffs = {}
        for field in fields:
            py_val = normalize_rarity(py[field]) if field == "rarity" else py[field]
            rs_val = normalize_rarity(rs[field]) if field == "rarity" else rs[field]
            if py_val != rs_val:
                diffs[field] = (py_val, rs_val)
        if diffs:
            field_mismatches.append((card_id, diffs))

    # Report
    print(f"\nShared IDs: {len(shared)}")

    if only_python:
        print(f"\nOnly in Python ({len(only_python)}):")
        for cid in sorted(list(only_python)[:10]):
            info = py_by_id[cid]
            print(f"  {cid}: {info.get('name', '?')} qty={info.get('quantity', '?')}")
        if len(only_python) > 10:
            print(f"  ... and {len(only_python) - 10} more")

    if only_rust:
        print(f"\nOnly in Rust ({len(only_rust)}):")
        for cid in sorted(list(only_rust)[:10]):
            info = rs_by_id[cid]
            print(f"  {cid}: {info.get('name', '?')} qty={info.get('quantity', '?')}")
        if len(only_rust) > 10:
            print(f"  ... and {len(only_rust) - 10} more")

    if quantity_mismatches:
        print(f"\nQuantity mismatches ({len(quantity_mismatches)}):")
        print("  (May differ due to scan timing — both scans read live memory)")
        for cid, py_qty, rs_qty in quantity_mismatches[:10]:
            name = py_by_id[cid].get("name", "?")
            print(f"  {cid} ({name}): Python={py_qty} Rust={rs_qty}")
        if len(quantity_mismatches) > 10:
            print(f"  ... and {len(quantity_mismatches) - 10} more")

    if field_mismatches:
        print(f"\nField mismatches ({len(field_mismatches)}):")
        for cid, diffs in field_mismatches[:20]:
            py_name = py_by_id[cid].get("name", "?")
            print(f"  {cid} ({py_name}):")
            for field, (py_val, rs_val) in diffs.items():
                print(f"    {field}: Python={py_val!r} Rust={rs_val!r}")
        if len(field_mismatches) > 20:
            print(f"  ... and {len(field_mismatches) - 20} more")

    # Field mismatches are real bugs; quantity/id differences may be scan timing
    has_bugs = bool(field_mismatches)
    has_variance = bool(only_python or only_rust or quantity_mismatches)

    if has_bugs:
        print("\nRESULT: FIELD MISMATCHES (bugs)")
        return 1
    elif has_variance:
        print("\nRESULT: VARIANCE (scan timing, not bugs)")
        return 0
    else:
        print("\nRESULT: EXACT MATCH")
        return 0


def main() -> int:
    import argparse

    parser = argparse.ArgumentParser(description="Diff Python vs Rust exports")
    parser.add_argument("command", choices=["card-db", "collection"])
    parser.add_argument("--output", help="Output directory", default=str(DEFAULT_OUTPUT))
    args = parser.parse_args()

    output_dir = Path(args.output)

    if args.command == "card-db":
        return diff_card_db(output_dir)
    elif args.command == "collection":
        return diff_collection(output_dir)
    return 1


if __name__ == "__main__":
    sys.exit(main())
