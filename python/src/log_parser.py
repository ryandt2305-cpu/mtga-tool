"""Parse MTGA Player.log for deck card IDs and non-collectible list."""
from __future__ import annotations

import json
import os
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True, slots=True)
class LogData:
    """Data extracted from Player.log."""

    known_owned_ids: set[int]  # Card IDs from user-created decks (soft anchors)
    non_collectible_ids: set[int]  # IDs to exclude from collection scan
    deck_cards: dict[int, int]  # card_id → max quantity across user decks


def find_player_log() -> Path:
    """Find MTGA Player.log."""
    local_low = os.environ.get("LOCALAPPDATA", "")
    if local_low:
        # LOCALAPPDATA points to Local, we need LocalLow
        local_low = Path(local_low).parent / "LocalLow"
    else:
        local_low = Path.home() / "AppData" / "LocalLow"

    log_path = local_low / "Wizards Of The Coast" / "MTGA" / "Player.log"
    if not log_path.exists():
        raise FileNotFoundError(f"Player.log not found at {log_path}")
    return log_path


def _is_precon(summary: dict) -> bool:
    """Check if a deck summary represents a precon deck."""
    name = summary.get("Name", "")
    desc = summary.get("Description", "")
    return "?=?" in name or "Loc/" in name or "Precon" in desc


def _extract_deck_card_ids(deck: dict) -> dict[int, int]:
    """Extract card_id → quantity from a deck's main deck + sideboard."""
    cards: dict[int, int] = {}
    for zone in ("MainDeck", "Sideboard", "CommandZone", "Companions"):
        for entry in deck.get(zone, []):
            cid = entry.get("cardId", 0)
            qty = entry.get("quantity", 0)
            if cid and qty:
                cards[cid] = max(cards.get(cid, 0), qty)
    return cards


def parse_player_log(log_path: Path) -> LogData:
    """Parse Player.log for the StartHook response.

    Extracts:
    - Card IDs from user-created (non-precon) decks as "soft anchors"
    - NonCollectibleCardList for exclusion during scanning
    """
    with open(log_path, "r", encoding="utf-8", errors="replace") as f:
        for line in f:
            if "InventoryInfo" not in line:
                continue

            line = line.strip()
            try:
                data = json.loads(line)
            except json.JSONDecodeError:
                continue

            if "InventoryInfo" not in data:
                continue

            # Build summary lookup: DeckId → summary
            summaries_list = data.get("DeckSummaries", [])
            summaries: dict[str, dict] = {}
            for s in summaries_list:
                summaries[s.get("DeckId", "")] = s

            # Extract card IDs from user decks only
            decks = data.get("Decks", {})
            all_deck_cards: dict[int, int] = {}

            for deck_id, deck in decks.items():
                summary = summaries.get(deck_id, {})
                if _is_precon(summary):
                    continue
                for cid, qty in _extract_deck_card_ids(deck).items():
                    all_deck_cards[cid] = max(all_deck_cards.get(cid, 0), qty)

            # Extract non-collectible card list
            meta = data.get("CardMetadataInfo", {})
            non_collectible = set(meta.get("NonCollectibleCardList", []))

            return LogData(
                known_owned_ids=set(all_deck_cards.keys()),
                non_collectible_ids=non_collectible,
                deck_cards=all_deck_cards,
            )

    # If we never found the StartHook line, return empty data
    return LogData(
        known_owned_ids=set(),
        non_collectible_ids=set(),
        deck_cards={},
    )
