"""Real-time Player.log watcher for discovering MTGA events.

Tails Player.log and surfaces structured events as they happen.
Useful for development — discovering what MTGA logs during pack opens,
match rewards, daily quests, store purchases, drafts, etc.
"""
from __future__ import annotations

import json
import os
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

from .card_database import CardInfo
from .log_parser import find_player_log


@dataclass
class LogEvent:
    """A parsed event from Player.log."""

    line_number: int
    event_type: str  # "json", "text", "json_invalid"
    raw: str
    data: dict | None = None  # Parsed JSON if valid
    keys: list[str] = field(default_factory=list)  # Top-level keys for JSON events


# Well-known event signatures (top-level key combinations → friendly names)
_EVENT_NAMES: dict[frozenset[str], str] = {
    frozenset({"InventoryInfo", "DeckSummaries", "Decks"}): "StartHook",
    frozenset({"Course", "InventoryInfo"}): "InventoryUpdate",
    frozenset({"transactionId", "requestId", "timestamp"}): "APIResponse",
    frozenset({"timestamp", "greToClientEvent"}): "GameEvent",
    frozenset({"Courses"}): "CourseList",
    frozenset({"NodeStates", "MilestoneStates"}): "ProgressUpdate",
    frozenset({"canSwap", "quests"}): "QuestUpdate",
    frozenset({"currentSeason", "limitedRankInfo", "constructedRankInfo"}): "RankInfo",
    frozenset({"DeckId", "Name", "Attributes"}): "DeckUpdate",
    frozenset({"CourseId", "InternalEventName", "CurrentModule"}): "EventProgress",
    frozenset({"MatchesV3"}): "MatchHistory",
    frozenset({"CurrentModule", "Payload"}): "ModulePayload",
    frozenset({"Formats", "FormatGroups"}): "FormatList",
    frozenset({"CacheVersion", "PreconDecks"}): "PreconDecks",
    frozenset({"Summaries"}): "DeckSummaries",
}


def _classify_event(data: dict) -> str:
    """Return a friendly name for a JSON event based on its top-level keys."""
    keys = frozenset(data.keys())
    # Check exact matches first
    if keys in _EVENT_NAMES:
        return _EVENT_NAMES[keys]
    # Check subset matches (event has extra keys beyond the signature)
    for sig, name in _EVENT_NAMES.items():
        if sig.issubset(keys):
            return name
    return "Unknown"


def _extract_highlights(
    data: dict,
    cards: dict[int, CardInfo] | None = None,
) -> list[str]:
    """Extract notable information from an event for display."""
    highlights = []

    # Inventory changes
    inv = data.get("InventoryInfo", {})
    changes = inv.get("Changes", [])
    for change in changes:
        source = change.get("Source", "?")
        parts = [f"Source={source}"]

        granted = change.get("GrantedCards", [])
        if granted:
            parts.append(f"Cards={len(granted)}")
            for card in granted[:3]:
                grp_id = card.get("GrpId", 0)
                set_code = card.get("SetCode", "?")
                info = cards.get(grp_id) if cards else None
                name = info.name if info else str(grp_id)
                parts.append(f"  +{name} ({set_code})")

        for currency in ("Gold", "Gems"):
            val = change.get(currency, 0)
            if val:
                parts.append(f"{currency}={val:+d}")

        for wc in ("WildCardCommons", "WildCardUnCommons", "WildCardRares", "WildCardMythics"):
            val = change.get(wc, 0)
            if val:
                short = wc.replace("WildCard", "WC_")
                parts.append(f"{short}={val:+d}")

        boosters = change.get("Boosters", [])
        if boosters:
            parts.append(f"Boosters={boosters}")

        vault = change.get("TotalVaultProgress", 0)
        if vault:
            parts.append(f"Vault={vault:+d}")

        xp = change.get("SetMasteryProgress", 0)
        if xp:
            parts.append(f"XP={xp:+d}")

        if len(parts) > 1:  # More than just Source
            highlights.append(" | ".join(parts))

    # Current currency snapshot (from InventoryInfo itself)
    if inv and not changes:
        currencies = []
        for key in ("Gold", "WildCardCommons", "WildCardUnCommons", "WildCardMythics"):
            val = inv.get(key)
            if val is not None:
                currencies.append(f"{key}={val}")
        if currencies:
            highlights.append("Currencies: " + ", ".join(currencies))

    # Quest updates
    quests = data.get("quests", [])
    if quests:
        for q in quests[:3]:
            highlights.append(f"Quest: {q.get('questId', '?')} progress={q.get('goal', '?')}")

    return highlights


def parse_log_line(raw: str, line_number: int) -> LogEvent | None:
    """Parse a single log line into a LogEvent."""
    stripped = raw.strip()
    if not stripped:
        return None

    if stripped.startswith("{"):
        try:
            data = json.loads(stripped)
            if isinstance(data, dict):
                return LogEvent(
                    line_number=line_number,
                    event_type="json",
                    raw=stripped,
                    data=data,
                    keys=list(data.keys()),
                )
        except json.JSONDecodeError:
            return LogEvent(
                line_number=line_number,
                event_type="json_invalid",
                raw=stripped,
            )

    return LogEvent(
        line_number=line_number,
        event_type="text",
        raw=stripped,
    )


def format_event(
    event: LogEvent,
    verbose: bool = False,
    cards: dict[int, CardInfo] | None = None,
) -> str | None:
    """Format an event for display. Returns None to skip."""
    if event.event_type == "text":
        return f"[TEXT] {event.raw[:200]}"

    if event.event_type == "json_invalid":
        return f"[JSON?] {event.raw[:200]}"

    if event.data is None:
        return None

    name = _classify_event(event.data)
    size = len(event.raw)
    lines = [f"[{name}] keys={event.keys[:5]} ({size:,} bytes)"]

    highlights = _extract_highlights(event.data, cards)
    for h in highlights:
        lines.append(f"  >>> {h}")

    if verbose:
        lines.append(json.dumps(event.data, indent=2)[:2000])

    return "\n".join(lines)


def tail_log(
    log_path: Path | None = None,
    *,
    filter_fn: Callable[[LogEvent], bool] | None = None,
    formatter: Callable[[LogEvent], str | None] | None = None,
    cards: dict[int, CardInfo] | None = None,
    poll_interval: float = 0.5,
    from_start: bool = False,
) -> None:
    """Tail Player.log and print events as they appear.

    Args:
        log_path: Path to Player.log. Auto-discovered if None.
        filter_fn: Optional predicate — only events where this returns True are shown.
        formatter: Optional custom formatter. Defaults to format_event.
        cards: Optional card database for resolving GrpIds to names.
        poll_interval: Seconds between file polls.
        from_start: If True, process from beginning of file. Otherwise start at end.
    """
    if log_path is None:
        log_path = find_player_log()

    if formatter is None:
        _cards = cards
        formatter = lambda event: format_event(event, cards=_cards)

    with open(log_path, "r", encoding="utf-8", errors="replace") as f:
        if not from_start:
            f.seek(0, os.SEEK_END)
            print(f"Watching {log_path.name} (tailing from end, {f.tell():,} bytes)")
        else:
            print(f"Watching {log_path.name} (from start)")

        print("Press Ctrl+C to stop.\n")

        line_number = 0
        try:
            while True:
                line = f.readline()
                if not line:
                    time.sleep(poll_interval)
                    continue

                line_number += 1
                event = parse_log_line(line, line_number)
                if event is None:
                    continue

                if filter_fn and not filter_fn(event):
                    continue

                output = formatter(event)
                if output:
                    print(output)
                    print()

        except KeyboardInterrupt:
            print(f"\nStopped after {line_number} lines.")
