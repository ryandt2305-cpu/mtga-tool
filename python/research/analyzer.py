"""Memory dump analysis, pointer detection, and StartHook comparison.

Provides tools for interpreting raw memory dumps:
- Hex dump with i32/i64 annotations
- Pointer detection (valid heap addresses)
- Annotation against known StartHook values
- Extended dump with pointer following
"""
from __future__ import annotations

import struct
from dataclasses import dataclass

import pymem

from .starthook import StartHookValues


@dataclass
class PointerCandidate:
    """A potential pointer found in a memory dump."""
    offset: int          # Offset from dump start
    address: int         # The pointer value (target address)
    target_preview: bytes | None  # First 32 bytes at target, or None if unreadable


def hex_dump(
    data: bytes,
    base_address: int,
    known_values: dict[str, int] | None = None,
    bytes_per_line: int = 16,
) -> str:
    """Format a memory dump as annotated hex + i32 values.

    Shows each 4-byte word as hex and decimal, with markers for known values.
    """
    lines: list[str] = []
    known_lookup: dict[int, str] = {}
    if known_values:
        for name, value in known_values.items():
            known_lookup[value] = name

    # Header
    lines.append(f"{'Address':>14s}  {'Hex':>10s}  {'i32':>12s}  {'f64 (if aligned)':>18s}  Annotation")
    lines.append("-" * 80)

    for i in range(0, len(data) - 3, 4):
        addr = base_address + i
        val_i32 = struct.unpack_from("<i", data, i)[0]
        val_u32 = struct.unpack_from("<I", data, i)[0]

        # Try f64 at 8-byte aligned positions
        f64_str = ""
        if i % 8 == 0 and i + 8 <= len(data):
            val_f64 = struct.unpack_from("<d", data, i)[0]
            if 0.0 < abs(val_f64) < 1e10 and val_f64 == val_f64:  # not NaN
                f64_str = f"{val_f64:.4f}"

        # Annotation
        annotation = ""
        if val_i32 in known_lookup:
            annotation = f"<-- {known_lookup[val_i32]}"

        lines.append(
            f"  {addr:#014x}  {val_u32:#010x}  {val_i32:>12,}  {f64_str:>18s}  {annotation}"
        )

    return "\n".join(lines)


def detect_pointers(
    data: bytes,
    pm: pymem.Pymem | None = None,
    min_addr: int = 0x1_0000_0000,
    max_addr: int = 0x7FFF_FFFF_FFFF,
) -> list[PointerCandidate]:
    """Find 64-bit values in data that look like valid heap pointers.

    Checks 8-byte aligned positions for values in the typical heap range.
    If pm is provided, attempts to read 32 bytes at each candidate target.
    """
    candidates: list[PointerCandidate] = []

    for i in range(0, len(data) - 7, 8):
        val = struct.unpack_from("<Q", data, i)[0]

        # Check if it's in a plausible heap address range
        if min_addr <= val <= max_addr and val % 8 == 0:  # .NET objects are 8-byte aligned
            preview = None
            if pm is not None:
                try:
                    preview = pm.read_bytes(val, 32)
                except Exception:
                    continue  # Not a valid pointer — skip

            candidates.append(PointerCandidate(
                offset=i,
                address=val,
                target_preview=preview,
            ))

    return candidates


def compare_with_starthook(
    scalars: dict[str, int | float],
    starthook: StartHookValues,
) -> str:
    """Compare memory-read inventory scalars against StartHook values.

    Returns a formatted comparison showing matches and mismatches.
    """
    lines: list[str] = []
    lines.append("Memory vs StartHook Comparison")
    lines.append("=" * 50)

    comparisons = [
        ("gold", scalars.get("gold"), starthook.gold),
        ("gems", scalars.get("gems"), starthook.gems),
        ("wc_common", scalars.get("wc_common"), starthook.wc_common),
        ("wc_uncommon", scalars.get("wc_uncommon"), starthook.wc_uncommon),
        ("wc_rare", scalars.get("wc_rare"), starthook.wc_rare),
        ("wc_mythic", scalars.get("wc_mythic"), starthook.wc_mythic),
        ("wc_track", scalars.get("wc_track_position"), starthook.wc_track_position),
        ("vault_raw", int(scalars.get("vault_progress", 0) * 10), starthook.vault_progress_raw),
    ]

    matches = 0
    total = 0
    for name, mem_val, log_val in comparisons:
        total += 1
        if mem_val == log_val:
            status = "MATCH"
            matches += 1
        else:
            status = "MISMATCH"
        lines.append(f"  {name:15s}  memory={str(mem_val):>8s}  log={str(log_val):>8s}  {status}")

    lines.append(f"\n  Result: {matches}/{total} fields match")
    if matches < total:
        lines.append("  NOTE: Mismatches may be due to in-game changes since StartHook.")

    return "\n".join(lines)


def annotate_extended_dump(
    data: bytes,
    base_address: int,
    starthook: StartHookValues,
    pm: pymem.Pymem | None = None,
) -> str:
    """Produce an annotated dump of memory beyond the known 40-byte scalar region.

    Shows: offset, hex, i32, potential pointer targets, and annotations for
    values that match known StartHook data (custom token counts, booster counts, etc.).
    """
    lines: list[str] = []

    # Build lookup of ALL known integer values from StartHook
    known_ints: dict[int, str] = {}

    # Token counts
    for name, count in starthook.custom_tokens.items():
        if count != 0:
            known_ints[count] = f"token:{name}"

    # Booster collation IDs and counts
    for collation_id, set_code, count in starthook.boosters:
        known_ints[collation_id] = f"booster_collation:{set_code}"
        if count != 0 and count not in known_ints:
            known_ints[count] = f"booster_count:{set_code}"

    # Cosmetics counts
    for name, count in [
        ("art_styles", starthook.art_styles_count),
        ("avatars", starthook.avatars_count),
        ("pets", starthook.pets_count),
        ("sleeves", starthook.sleeves_count),
        ("emotes", starthook.emotes_count),
        ("titles", starthook.titles_count),
    ]:
        if count != 0:
            known_ints[count] = f"cosmetics:{name}"

    # Rank values
    for name, val in [
        ("constructed_level", starthook.constructed_level),
        ("constructed_step", starthook.constructed_step),
        ("limited_level", starthook.limited_level),
        ("limited_step", starthook.limited_step),
        ("season_ordinal", starthook.season_ordinal),
    ]:
        if val != 0:
            known_ints[val] = f"rank:{name}"

    lines.append(f"Extended dump: {len(data)} bytes from {base_address:#x}")
    lines.append(f"Known values to match: {len(known_ints)}")
    lines.append("")
    lines.append(f"{'Offset':>8s}  {'Address':>14s}  {'Hex':>10s}  {'i32':>12s}  Annotation")
    lines.append("-" * 80)

    for i in range(0, len(data) - 3, 4):
        addr = base_address + i
        val_i32 = struct.unpack_from("<i", data, i)[0]
        val_u32 = struct.unpack_from("<I", data, i)[0]

        annotation = ""
        if val_i32 in known_ints:
            annotation = f"<-- {known_ints[val_i32]} = {val_i32}"

        # Check for pointer-like values at 8-byte aligned positions
        if i % 8 == 0 and i + 8 <= len(data):
            ptr_val = struct.unpack_from("<Q", data, i)[0]
            if 0x1_0000_0000 <= ptr_val <= 0x7FFF_FFFF_FFFF and ptr_val % 8 == 0:
                annotation += f"  [PTR? -> {ptr_val:#x}]"

        if annotation:
            lines.append(
                f"  {i:>6d}  {addr:#014x}  {val_u32:#010x}  {val_i32:>12,}  {annotation}"
            )
        else:
            lines.append(
                f"  {i:>6d}  {addr:#014x}  {val_u32:#010x}  {val_i32:>12,}"
            )

    # Detect and display pointer candidates
    if pm is not None:
        pointers = detect_pointers(data, pm)
        if pointers:
            lines.append("")
            lines.append(f"Pointer candidates: {len(pointers)}")
            lines.append("-" * 60)
            for p in pointers:
                preview_hex = ""
                if p.target_preview:
                    preview_hex = " ".join(f"{b:02x}" for b in p.target_preview[:16])
                lines.append(
                    f"  offset={p.offset:>4d}  -> {p.address:#014x}  [{preview_hex}]"
                )

    return "\n".join(lines)
