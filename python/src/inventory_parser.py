"""Extract full player inventory snapshot from MTGA's Player.log."""
from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path

from .log_parser import _is_precon, find_player_log


@dataclass(frozen=True, slots=True)
class BoosterStack:
    """A stack of boosters for a single collation."""

    collation_id: int
    count: int


@dataclass(slots=True)
class PlayerInventory:
    """Full inventory state extracted from the StartHook response."""

    gold: int
    gems: int
    vault_progress: float
    wc_common: int
    wc_uncommon: int
    wc_rare: int
    wc_mythic: int
    wc_track_position: int
    draft_tokens: int
    sealed_tokens: int
    boosters: list[BoosterStack]
    total_boosters: int
    custom_tokens: dict[str, int]
    user_deck_count: int
    precon_deck_count: int


def parse_inventory(log_path: Path | None = None) -> PlayerInventory | None:
    """Parse Player.log and extract the full inventory snapshot.

    Returns None if the log can't be found or doesn't contain the StartHook blob.
    """
    if log_path is None:
        try:
            log_path = find_player_log()
        except FileNotFoundError:
            return None

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

            inv = data["InventoryInfo"]

            # Boosters
            boosters = [
                BoosterStack(collation_id=b["CollationId"], count=b["Count"])
                for b in inv.get("Boosters", [])
            ]
            total_boosters = sum(b.count for b in boosters)

            # Deck counts
            summaries_list = data.get("DeckSummaries", [])
            user_count = 0
            precon_count = 0
            for s in summaries_list:
                if _is_precon(s):
                    precon_count += 1
                else:
                    user_count += 1

            # Draft/Sealed tokens: may be top-level or in CustomTokens
            custom_tokens = inv.get("CustomTokens", {}) or {}
            draft_tokens = inv.get("DraftTokens", 0) or custom_tokens.get("DraftToken", 0)
            sealed_tokens = inv.get("SealedTokens", 0) or custom_tokens.get("SealedToken", 0)

            return PlayerInventory(
                gold=inv.get("Gold", 0),
                gems=inv.get("Gems", 0),
                vault_progress=inv.get("TotalVaultProgress", 0) / 10.0,
                wc_common=inv.get("WildCardCommons", 0),
                wc_uncommon=inv.get("WildCardUnCommons", 0),
                wc_rare=inv.get("WildCardRares", 0),
                wc_mythic=inv.get("WildCardMythics", 0),
                wc_track_position=inv.get("wcTrackPosition", 0),
                draft_tokens=draft_tokens,
                sealed_tokens=sealed_tokens,
                boosters=boosters,
                total_boosters=total_boosters,
                custom_tokens=custom_tokens,
                user_deck_count=user_count,
                precon_deck_count=precon_count,
            )

    return None


def format_inventory(inv: PlayerInventory) -> str:
    """Format a PlayerInventory as a human-readable multi-line summary."""
    lines = [
        "  Currencies",
        f"    Gold:      {inv.gold:,}",
        f"    Gems:      {inv.gems:,}",
        "",
        "  Wildcards",
        f"    Common:    {inv.wc_common}",
        f"    Uncommon:  {inv.wc_uncommon}",
        f"    Rare:      {inv.wc_rare}",
        f"    Mythic:    {inv.wc_mythic}",
        f"    Track pos: {inv.wc_track_position}",
        "",
        "  Vault",
        f"    Progress:  {inv.vault_progress:.1f}%",
        "",
        f"  Boosters ({inv.total_boosters} total)",
    ]

    for b in inv.boosters:
        lines.append(f"    [{b.collation_id}]: {b.count} packs")

    if not inv.boosters:
        lines.append("    (none)")

    lines += [
        "",
        "  Tokens",
        f"    Draft:     {inv.draft_tokens}",
        f"    Sealed:    {inv.sealed_tokens}",
    ]

    if inv.custom_tokens:
        lines.append("")
        lines.append("  Custom Tokens")
        for token_id, count in sorted(inv.custom_tokens.items()):
            lines.append(f"    {token_id}: {count}")

    lines += [
        "",
        "  Decks",
        f"    User:      {inv.user_deck_count}",
        f"    Precon:    {inv.precon_deck_count}",
    ]

    return "\n".join(lines)
