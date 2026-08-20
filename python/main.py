"""MTGA Collection Tool — CLI entry point.

Subcommands:
    collection  Scan MTGA memory and export card collection (default)
    inventory   Show player inventory from Player.log
    watch       Tail Player.log for real-time events
"""
from __future__ import annotations

import argparse
import sys
import time
from collections import Counter
from pathlib import Path

from src.card_database import CardInfo, find_card_database, find_mtga_install, load_card_database
from src.log_parser import LogData, find_player_log, parse_player_log

OUTPUT_DIR = Path(__file__).parent / "output"


# ── collection subcommand ────────────────────────────────────────────


def print_summary(
    collection: dict[int, int],
    cards: dict[int, CardInfo],
    log_data: LogData,
) -> None:
    """Print collection summary statistics."""
    total_cards = sum(collection.values())
    unique_cards = len(collection)

    rarity_counts: Counter[str] = Counter()
    for card_id in collection:
        info = cards.get(card_id)
        if info:
            rarity_counts[info.rarity] += 1

    print(f"\n{'='*50}")
    print(f"  Collection Summary")
    print(f"{'='*50}")
    print(f"  Unique cards:  {unique_cards:,}")
    print(f"  Total copies:  {total_cards:,}")
    print()

    rarity_order = ["mythic", "rare", "uncommon", "common", "basic_land", "special"]
    for rarity in rarity_order:
        count = rarity_counts.get(rarity, 0)
        if count:
            label = rarity.replace("_", " ").title()
            print(f"  {label:>12}: {count:,}")

    unknown = sum(1 for cid in collection if cid not in cards)
    if unknown:
        print(f"  {'Unknown':>12}: {unknown:,}")

    # Anchor validation
    if log_data.known_owned_ids:
        found = sum(1 for cid in log_data.known_owned_ids if cid in collection)
        total = len(log_data.known_owned_ids)
        pct = found / total * 100 if total else 0
        print(f"\n  Deck validation: {found}/{total} known cards found ({pct:.0f}%)")

        # Check quantity mismatches
        mismatches = 0
        for cid, expected_qty in log_data.deck_cards.items():
            actual_qty = collection.get(cid, 0)
            if actual_qty < expected_qty:
                mismatches += 1
        if mismatches:
            print(f"  Quantity warnings: {mismatches} cards have fewer copies than in decks")

    print(f"{'='*50}")


def cmd_collection(args: argparse.Namespace) -> int:
    """Scan MTGA memory and export card collection."""
    import pymem.exception

    from src.exporter import export_all
    from src.memory_scanner import scan_collection

    print("MTGA Collection Exporter")
    print("========================\n")

    # 1. Find MTGA install and load card database
    print("[1/4] Loading card database...")
    try:
        install_dir = find_mtga_install()
        print(f"  MTGA install: {install_dir}")
    except FileNotFoundError as e:
        print(f"  ERROR: {e}", file=sys.stderr)
        return 1

    try:
        db_path = find_card_database(install_dir)
        cards, valid_ids = load_card_database(db_path)
        print(f"  Loaded {len(cards):,} cards from {db_path.name}")
    except Exception as e:
        print(f"  ERROR: {e}", file=sys.stderr)
        return 1

    # 2. Parse Player.log for validation data
    print("\n[2/4] Parsing Player.log...")
    try:
        log_path = find_player_log()
        log_data = parse_player_log(log_path)
        print(f"  Found {len(log_data.known_owned_ids)} card IDs from "
              f"{len(log_data.deck_cards)} user-deck entries")
        print(f"  {len(log_data.non_collectible_ids)} non-collectible IDs to exclude")
    except FileNotFoundError:
        print("  Player.log not found — continuing without validation data")
        log_data = LogData(set(), set(), {})

    # 3. Scan memory
    print("\n[3/4] Scanning MTGA process memory...")
    try:
        start = time.perf_counter()
        result = scan_collection(
            valid_ids=valid_ids,
            anchor_ids=log_data.known_owned_ids,
            non_collectible_ids=log_data.non_collectible_ids,
            progress_callback=lambda cur, tot: print(
                f"\r  Scanning region {cur}/{tot}...", end="", flush=True
            ),
        )
        elapsed = time.perf_counter() - start
        print(f"\r  Scan complete ({elapsed:.1f}s)                    ")
    except pymem.exception.ProcessNotFound:
        print("  ERROR: MTGA.exe is not running. Launch the game first.", file=sys.stderr)
        return 1
    except RuntimeError as e:
        print(f"  ERROR: {e}", file=sys.stderr)
        return 1

    collection = result.collection

    # Print summary
    print_summary(collection, cards, log_data)

    # Cross-validation report
    if result.cross_validation:
        cv = result.cross_validation
        print(f"\n  Cross-validation (best vs runner-up):")
        print(f"    Cards in common:   {cv.shared_ids}")
        if cv.shared_ids:
            print(f"    Quantity matches:  {cv.quantity_matches}/{cv.shared_ids} "
                  f"({cv.quantity_matches / cv.shared_ids * 100:.0f}%)")
        if cv.quantity_mismatches:
            print(f"    Quantity differs:  {cv.quantity_mismatches}")
        if cv.only_in_a:
            print(f"    Only in best:      {cv.only_in_a}")
        if cv.only_in_b:
            print(f"    Only in runner-up: {cv.only_in_b}")

    # 4. Export
    output_dir = Path(args.output) if args.output else OUTPUT_DIR
    print(f"\n[4/4] Exporting to {output_dir}/...")
    paths = export_all(collection, cards, output_dir)
    for p in paths:
        print(f"  {p.name}")

    print("\nDone.")
    return 0


# ── inventory subcommand ─────────────────────────────────────────────


def cmd_inventory(args: argparse.Namespace) -> int:
    """Show player inventory from Player.log."""
    from src.inventory_parser import format_inventory, parse_inventory

    log_path = Path(args.log) if args.log else None
    inv = parse_inventory(log_path)

    if inv is None:
        print("ERROR: Could not parse inventory from Player.log.", file=sys.stderr)
        print("Make sure MTGA has been launched at least once.", file=sys.stderr)
        return 1

    print("MTGA Player Inventory")
    print("=====================\n")
    print(format_inventory(inv))

    if args.json:
        import json
        from dataclasses import asdict

        output_dir = Path(args.output) if args.output else OUTPUT_DIR
        output_dir.mkdir(parents=True, exist_ok=True)
        out_path = output_dir / "inventory.json"
        with open(out_path, "w", encoding="utf-8") as f:
            json.dump(asdict(inv), f, indent=2)
        print(f"\n  Saved to {out_path}")

    return 0


# ── watch subcommand ─────────────────────────────────────────────────


def cmd_watch(args: argparse.Namespace) -> int:
    """Tail Player.log for real-time events."""
    from src.log_watcher import LogEvent, format_event, tail_log

    # Load card database for name resolution
    cards = None
    try:
        install_dir = find_mtga_install()
        db_path = find_card_database(install_dir)
        cards, _ = load_card_database(db_path)
        print(f"Loaded {len(cards):,} cards for name resolution\n")
    except (FileNotFoundError, Exception):
        print("Card database not found — GrpIds will be shown as numbers\n")

    def filter_fn(event: LogEvent) -> bool:
        if not args.all and event.event_type == "text":
            return False
        if args.inventory:
            if event.data is None:
                return False
            inv = event.data.get("InventoryInfo", {})
            if not inv:
                return False
            changes = inv.get("Changes", [])
            has_reward_data = any(
                change.get("GrantedCards") or change.get("Gold") or
                change.get("Gems") or change.get("Boosters") or
                change.get("WildCardCommons") or change.get("WildCardRares") or
                change.get("WildCardMythics") or change.get("TotalVaultProgress")
                for change in changes
            )
            return has_reward_data or "DeckSummaries" in event.data
        if args.keyword:
            return args.keyword.lower() in event.raw.lower()
        return True

    def formatter(event: LogEvent) -> str | None:
        return format_event(event, verbose=args.verbose, cards=cards)

    tail_log(
        filter_fn=filter_fn,
        formatter=formatter,
        cards=cards,
        from_start=args.from_start,
    )
    return 0


# ── CLI setup ────────────────────────────────────────────────────────


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="mtga-tool",
        description="MTGA Collection Tool — export collections, view inventory, monitor events",
    )
    sub = parser.add_subparsers(dest="command")

    # collection (default)
    p_col = sub.add_parser("collection", help="Scan MTGA memory and export card collection")
    p_col.add_argument("-o", "--output", help="Output directory (default: ./output)")

    # inventory
    p_inv = sub.add_parser("inventory", help="Show player inventory from Player.log")
    p_inv.add_argument("--log", help="Path to Player.log (auto-discovered if omitted)")
    p_inv.add_argument("--json", action="store_true", help="Also export inventory as JSON")
    p_inv.add_argument("-o", "--output", help="Output directory for JSON export")

    # watch
    p_watch = sub.add_parser("watch", help="Tail Player.log for real-time events")
    p_watch.add_argument("--all", action="store_true", help="Include plain text lines")
    p_watch.add_argument("-v", "--verbose", action="store_true", help="Show full JSON")
    p_watch.add_argument("-i", "--inventory", action="store_true",
                         help="Only inventory/reward events")
    p_watch.add_argument("-k", "--keyword", type=str, help="Filter by keyword")
    p_watch.add_argument("--from-start", action="store_true", help="Process from start of file")

    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()

    command = args.command or "collection"

    # When no subcommand is given, args won't have subcommand-specific attrs
    if command == "collection" and not hasattr(args, "output"):
        args.output = None
    if command == "inventory" and not hasattr(args, "log"):
        args.log = None
        args.json = False
        args.output = None

    dispatch = {
        "collection": cmd_collection,
        "inventory": cmd_inventory,
        "watch": cmd_watch,
    }

    handler = dispatch.get(command)
    if handler is None:
        parser.print_help()
        return 1

    return handler(args)


if __name__ == "__main__":
    sys.exit(main())
