"""Load MTGA card database from local SQLite file."""
from __future__ import annotations

import glob
import re
import sqlite3
from dataclasses import dataclass
from pathlib import Path

RARITY_MAP = {
    0: "special",
    1: "basic_land",
    2: "common",
    3: "uncommon",
    4: "rare",
    5: "mythic",
}

STEAM_DEFAULT = Path(r"C:\Program Files (x86)\Steam\steamapps\common\MTGA")
EPIC_DEFAULT = Path(r"C:\Program Files\Epic Games\MagicTheGathering")

SEARCH_PATHS = [STEAM_DEFAULT, EPIC_DEFAULT]


@dataclass(frozen=True, slots=True)
class CardInfo:
    grp_id: int
    name: str
    set_code: str
    rarity: str
    collector_number: str


_MARKUP_RE = re.compile(r"<[^>]+>")


def _clean_name(raw: str) -> str:
    """Strip MTGA markup tags from localized card names.

    The localization DB includes:
    - <nobr>...</nobr> around hyphenated words (e.g. Snow-Covered)
    - <sprite="..."> prefixes on Alchemy/rebalanced cards
    """
    return _MARKUP_RE.sub("", raw)


def find_mtga_install() -> Path:
    """Return MTGA install directory or raise."""
    for p in SEARCH_PATHS:
        if p.is_dir():
            return p
    raise FileNotFoundError(
        "MTGA install not found. Checked:\n"
        + "\n".join(f"  {p}" for p in SEARCH_PATHS)
    )


def find_card_database(install_dir: Path) -> Path:
    """Find Raw_CardDatabase_*.mtga inside the install directory."""
    raw_dir = install_dir / "MTGA_Data" / "Downloads" / "Raw"
    matches = glob.glob(str(raw_dir / "Raw_CardDatabase_*.mtga"))
    if not matches:
        raise FileNotFoundError(f"No card database found in {raw_dir}")
    # If multiple, take the first (there should only be one)
    return Path(matches[0])


def load_card_database(
    db_path: Path,
) -> tuple[dict[int, CardInfo], set[int]]:
    """Load card catalog from MTGA's SQLite card database.

    Returns:
        cards: mapping of GrpId → CardInfo for all non-token cards
        valid_ids: set of all non-token GrpIds (for fast membership checks)
    """
    conn = sqlite3.connect(str(db_path))
    try:
        rows = conn.execute(
            """
            SELECT c.GrpId, l.Loc, c.ExpansionCode, c.Rarity, c.CollectorNumber
            FROM Cards c
            JOIN Localizations_enUS l ON c.TitleId = l.LocId AND l.Formatted = 1
            WHERE c.IsToken = 0
            """
        ).fetchall()
    finally:
        conn.close()

    cards: dict[int, CardInfo] = {}
    for grp_id, name, set_code, rarity_int, collector_num in rows:
        name = _clean_name(name)
        cards[grp_id] = CardInfo(
            grp_id=grp_id,
            name=name,
            set_code=set_code,
            rarity=RARITY_MAP.get(rarity_int, f"unknown_{rarity_int}"),
            collector_number=collector_num or "",
        )

    valid_ids = set(cards.keys())
    return cards, valid_ids
