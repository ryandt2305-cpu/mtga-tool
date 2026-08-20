"""Debug tool — tail Player.log and surface MTGA events in real-time.

Usage:
    python watch.py                  # Tail from end, show all JSON events
    python watch.py --all            # Include plain text log lines
    python watch.py --verbose        # Show full JSON payloads
    python watch.py --inventory      # Only show inventory/reward events
    python watch.py --keyword WORD   # Only show events containing WORD
    python watch.py --from-start     # Process from beginning of log
"""
from __future__ import annotations

import argparse
import sys

from src.card_database import find_card_database, find_mtga_install, load_card_database
from src.log_watcher import LogEvent, format_event, tail_log


def main() -> int:
    parser = argparse.ArgumentParser(description="Watch MTGA Player.log events")
    parser.add_argument("--all", action="store_true", help="Include plain text lines")
    parser.add_argument("--verbose", "-v", action="store_true", help="Show full JSON")
    parser.add_argument("--inventory", "-i", action="store_true",
                        help="Only inventory/reward events")
    parser.add_argument("--keyword", "-k", type=str, help="Filter by keyword in raw line")
    parser.add_argument("--from-start", action="store_true", help="Process from start of file")
    args = parser.parse_args()

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
            # Show events with InventoryInfo that have Changes, or Course+Inventory
            inv = event.data.get("InventoryInfo", {})
            if not inv:
                return False
            changes = inv.get("Changes", [])
            # Show if there are actual changes, or if it's an initial snapshot
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


if __name__ == "__main__":
    sys.exit(main())
