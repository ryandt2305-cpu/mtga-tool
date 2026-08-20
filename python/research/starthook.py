"""Extract known values from the most recent StartHook response in Player.log.

Parses the last InventoryInfo blob to get all scalar values, custom tokens,
booster stacks, and cosmetics counts. These serve as search anchors and
validation targets for memory scanning.
"""
from __future__ import annotations

import json
from dataclasses import dataclass, field
from pathlib import Path


def _find_player_log() -> Path:
    """Find Player.log in the standard MTGA location."""
    appdata = Path.home() / "AppData" / "LocalLow" / "Wizards Of The Coast" / "MTGA"
    log = appdata / "Player.log"
    if log.exists():
        return log
    raise FileNotFoundError(f"Player.log not found at {log}")


@dataclass
class StartHookValues:
    """All known values from the most recent StartHook response."""

    # Scalars (matches Rust inventory.rs layout)
    gold: int = 0
    gems: int = 0
    wc_common: int = 0
    wc_uncommon: int = 0
    wc_rare: int = 0
    wc_mythic: int = 0
    wc_track_position: int = 0
    vault_progress_raw: int = 0  # TotalVaultProgress (raw, divide by 10 for %)

    # Custom tokens
    custom_tokens: dict[str, int] = field(default_factory=dict)

    # Boosters: list of (collation_id, set_code, count)
    boosters: list[tuple[int, str, int]] = field(default_factory=list)

    # Cosmetics counts
    art_styles_count: int = 0
    avatars_count: int = 0
    pets_count: int = 0
    sleeves_count: int = 0
    emotes_count: int = 0
    titles_count: int = 0

    # Rank (from RankGetCombinedRankInfo if present)
    constructed_level: int = 0
    constructed_step: int = 0
    constructed_wins: int = 0
    limited_level: int = 0
    limited_step: int = 0
    limited_wins: int = 0
    limited_losses: int = 0
    season_ordinal: int = 0


def extract_starthook(log_path: Path | None = None) -> StartHookValues:
    """Parse the LAST StartHook response from Player.log.

    Reads the file in reverse order to find the most recent StartHook quickly.
    Falls back to forward scan if reverse scan fails.
    """
    if log_path is None:
        log_path = _find_player_log()

    values = StartHookValues()
    last_inventory_line: str | None = None
    last_rank_line: str | None = None

    with open(log_path, "r", encoding="utf-8", errors="replace") as f:
        for line in f:
            stripped = line.strip()
            if not stripped.startswith("{"):
                continue

            # Find InventoryInfo (StartHook)
            if '"InventoryInfo"' in stripped and '"DeckSummaries"' in stripped:
                last_inventory_line = stripped

            # Find rank info
            if '"constructedSeasonOrdinal"' in stripped:
                last_rank_line = stripped

    # Parse the last StartHook
    if last_inventory_line:
        try:
            data = json.loads(last_inventory_line)
            inv = data.get("InventoryInfo", {})

            values.gold = inv.get("Gold", 0)
            values.gems = inv.get("Gems", 0)
            values.wc_common = inv.get("WildCardCommons", 0)
            values.wc_uncommon = inv.get("WildCardUnCommons", 0)
            values.wc_rare = inv.get("WildCardRares", 0)
            values.wc_mythic = inv.get("WildCardMythics", 0)
            values.wc_track_position = inv.get("wcTrackPosition", 0)
            values.vault_progress_raw = inv.get("TotalVaultProgress", 0)

            values.custom_tokens = inv.get("CustomTokens", {}) or {}

            for b in inv.get("Boosters", []):
                values.boosters.append((
                    b.get("CollationId", 0),
                    b.get("SetCode", ""),
                    b.get("Count", 0),
                ))

            cosmetics = inv.get("Cosmetics", {})
            if cosmetics:
                values.art_styles_count = len(cosmetics.get("ArtStyles", []))
                values.avatars_count = len(cosmetics.get("Avatars", []))
                values.pets_count = len(cosmetics.get("Pets", []))
                values.sleeves_count = len(cosmetics.get("Sleeves", []))
                values.emotes_count = len(cosmetics.get("Emotes", []))
                values.titles_count = len(cosmetics.get("Titles", []))

        except (json.JSONDecodeError, KeyError, TypeError):
            pass

    # Parse rank info
    if last_rank_line:
        try:
            rank = json.loads(last_rank_line)
            values.constructed_level = rank.get("constructedLevel", 0)
            values.constructed_step = rank.get("constructedStep", 0)
            values.constructed_wins = rank.get("constructedMatchesWon", 0)
            values.limited_level = rank.get("limitedLevel", 0)
            values.limited_step = rank.get("limitedStep", 0)
            values.limited_wins = rank.get("limitedMatchesWon", 0)
            values.limited_losses = rank.get("limitedMatchesLost", 0)
            values.season_ordinal = rank.get("constructedSeasonOrdinal", 0)
        except (json.JSONDecodeError, KeyError, TypeError):
            pass

    return values


def format_starthook(v: StartHookValues) -> str:
    """Format StartHookValues for display."""
    lines = [
        "StartHook Known Values",
        "=" * 40,
        "",
        "Inventory Scalars:",
        f"  Gold:           {v.gold:>8,}",
        f"  Gems:           {v.gems:>8,}",
        f"  WC Common:      {v.wc_common:>8}",
        f"  WC Uncommon:    {v.wc_uncommon:>8}",
        f"  WC Rare:        {v.wc_rare:>8}",
        f"  WC Mythic:      {v.wc_mythic:>8}",
        f"  WC Track:       {v.wc_track_position:>8}",
        f"  Vault (raw):    {v.vault_progress_raw:>8}  ({v.vault_progress_raw / 10:.1f}%)",
        "",
    ]

    if v.custom_tokens:
        lines.append("Custom Tokens:")
        for name, count in sorted(v.custom_tokens.items()):
            lines.append(f"  {name:30s} = {count}")
        lines.append("")

    if v.boosters:
        lines.append("Boosters:")
        for collation_id, set_code, count in v.boosters:
            lines.append(f"  [{collation_id}] {set_code:>4s}: {count} packs")
        lines.append("")

    lines.append("Cosmetics Counts:")
    lines.append(f"  Art Styles:     {v.art_styles_count:>4}")
    lines.append(f"  Avatars:        {v.avatars_count:>4}")
    lines.append(f"  Pets:           {v.pets_count:>4}")
    lines.append(f"  Sleeves:        {v.sleeves_count:>4}")
    lines.append(f"  Emotes:         {v.emotes_count:>4}")
    lines.append(f"  Titles:         {v.titles_count:>4}")
    lines.append("")

    if v.season_ordinal:
        lines.append("Rank:")
        lines.append(f"  Season:         {v.season_ordinal}")
        lines.append(f"  Constructed:    Level {v.constructed_level}, Step {v.constructed_step}, {v.constructed_wins}W")
        lines.append(f"  Limited:        Level {v.limited_level}, Step {v.limited_step}, {v.limited_wins}W/{v.limited_losses}L")

    return "\n".join(lines)
