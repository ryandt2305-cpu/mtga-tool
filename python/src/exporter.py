"""Export collection data to JSON, CSV, and TXT formats."""
from __future__ import annotations

import csv
import json
from pathlib import Path

from .card_database import CardInfo

# MTGA-only digital sets that Moxfield/Scryfall don't index.
# Cards from these sets are exported without set/CN so Moxfield resolves by name.
_MTGA_ONLY_SETS = {"J21", "Y22", "Y23", "Y24", "Y25", "HBG", "SPG", "PRM"}

_BASIC_NAMES = {"Plains", "Island", "Swamp", "Mountain", "Forest"}

# Cards where MTGA's collector number doesn't match Scryfall's.
# key = (set_code, collector_number), value = corrected (set_code, cn) or ("","") to drop.
_CN_OVERRIDES: dict[tuple[str, str], tuple[str, str]] = {
    ("OGW", "186"): ("OGW", "184"),  # Wastes full-art, MTGA uses different CN
}

# Threshold above which a basic land's collector number belongs to a commander
# product that MTGA merges into the main set code.
_COMMANDER_BASIC_CN_THRESHOLD = 300


def _moxfield_set_cn(info: CardInfo) -> tuple[str, str]:
    """Return (set_code, collector_number) adjusted for Moxfield compatibility.

    Drops set/CN for MTGA-only sets and commander-product basics so Moxfield
    falls back to name-only resolution.
    """
    if info.set_code in _MTGA_ONLY_SETS:
        return "", ""

    override = _CN_OVERRIDES.get((info.set_code, info.collector_number))
    if override:
        return override

    cn_int = int(info.collector_number) if info.collector_number.isdigit() else 0
    if info.name in _BASIC_NAMES and cn_int > _COMMANDER_BASIC_CN_THRESHOLD:
        return "", ""

    return info.set_code, info.collector_number


def export_json(
    collection: dict[int, int],
    cards: dict[int, CardInfo],
    output_path: Path,
) -> None:
    """Export as JSON array with full card details."""
    entries = []
    for card_id, quantity in sorted(collection.items()):
        info = cards.get(card_id)
        entries.append(
            {
                "id": card_id,
                "name": info.name if info else f"Unknown ({card_id})",
                "set": info.set_code if info else "",
                "rarity": info.rarity if info else "",
                "quantity": quantity,
                "collector_number": info.collector_number if info else "",
            }
        )

    with open(output_path, "w", encoding="utf-8") as f:
        json.dump(entries, f, indent=2, ensure_ascii=False)


def export_csv(
    collection: dict[int, int],
    cards: dict[int, CardInfo],
    output_path: Path,
) -> None:
    """Export as CSV (Moxfield collection import format).

    Moxfield expects: Count, Name, Edition, Condition, Language, Foil, Tag
    Moxfield identifies columns by header name, not position.
    """
    with open(output_path, "w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f)
        writer.writerow(["Count", "Name", "Edition", "Condition", "Language", "Foil", "Tag"])
        for card_id, quantity in sorted(collection.items()):
            info = cards.get(card_id)
            name = info.name if info else f"Unknown ({card_id})"
            edition = info.set_code if info else ""
            writer.writerow([quantity, name, edition, "NM", "English", "", ""])


def export_txt(
    collection: dict[int, int],
    cards: dict[int, CardInfo],
    output_path: Path,
) -> None:
    """Export as Moxfield bulk-edit format.

    Format: <amount> <name> (<set>) <collector_number>
    e.g.  1 Lightning Bolt (FCA) 185

    Cards from MTGA-only sets or with invalid set/CN combos are exported
    without set info so Moxfield resolves by name.
    """
    lines = []
    for card_id, quantity in sorted(collection.items()):
        info = cards.get(card_id)
        if not info:
            lines.append(f"{quantity} Unknown ({card_id})")
            continue

        set_code, cn = _moxfield_set_cn(info)
        if set_code:
            lines.append(f"{quantity} {info.name} ({set_code}) {cn}")
        else:
            lines.append(f"{quantity} {info.name}")

    with open(output_path, "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")


def export_all(
    collection: dict[int, int],
    cards: dict[int, CardInfo],
    output_dir: Path,
) -> list[Path]:
    """Export collection in all formats. Returns list of written files."""
    output_dir.mkdir(parents=True, exist_ok=True)
    paths = []

    json_path = output_dir / "collection.json"
    export_json(collection, cards, json_path)
    paths.append(json_path)

    csv_path = output_dir / "collection.csv"
    export_csv(collection, cards, csv_path)
    paths.append(csv_path)

    txt_path = output_dir / "collection.txt"
    export_txt(collection, cards, txt_path)
    paths.append(txt_path)

    return paths
