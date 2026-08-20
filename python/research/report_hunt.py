"""Hunt for ClientInventoryUpdateReportItem objects after an inventory mutation.

Strategy:
1. Poll inventory scalars rapidly to detect change
2. When change detected, compute deltas
3. Search globally for delta patterns that match InventoryDelta layout
4. Analyze hits for report item structure

Run from mtga-tool/: python -m python.research.report_hunt
"""
from __future__ import annotations

import argparse
import struct
import sys
import time

from .probe import (
    InventoryLocation,
    enumerate_regions,
    find_inventory,
    open_mtga,
    read_i32,
    read_i64,
    read_memory,
)
from .starthook import extract_starthook


def read_inventory_scalars(pm, addr: int) -> dict[str, int] | None:
    """Read the 40-byte scalar block at the inventory address."""
    data = read_memory(pm, addr, 40)
    if data is None:
        return None
    return {
        "wc_common": struct.unpack_from("<i", data, 0)[0],
        "wc_uncommon": struct.unpack_from("<i", data, 4)[0],
        "wc_rare": struct.unpack_from("<i", data, 8)[0],
        "wc_mythic": struct.unpack_from("<i", data, 12)[0],
        "gold": struct.unpack_from("<i", data, 16)[0],
        "gems": struct.unpack_from("<i", data, 20)[0],
        "wc_track": struct.unpack_from("<i", data, 24)[0],
        # vault_progress is f64 at offset 32
        "vault_raw": int(struct.unpack_from("<d", data, 32)[0] * 10),
    }


def read_booster_count(pm, inv_addr: int) -> int | None:
    """Read the booster list _size from inv-64 -> List -> _size at +24."""
    list_ptr = read_i64(pm, inv_addr - 64)
    if list_ptr is None or list_ptr == 0:
        return None
    size = read_i32(pm, list_ptr + 24)
    return size


def global_search_i32(pm, regions, value: int, *, max_hits: int = 50000) -> list[int]:
    """Search all memory for an i32 value. Returns list of addresses."""
    value_bytes = struct.pack("<i", value)
    chunk_size = 4 * 1024 * 1024
    hits = []

    for region in regions:
        offset = 0
        while offset < region.size:
            read_size = min(chunk_size, region.size - offset)
            try:
                data = pm.read_bytes(region.base + offset, read_size)
            except Exception:
                offset += read_size
                continue

            pos = 0
            while True:
                idx = data.find(value_bytes, pos)
                if idx == -1:
                    break
                hits.append(region.base + offset + idx)
                pos = idx + 1
                if len(hits) >= max_hits:
                    return hits

            offset += read_size

    return hits


def search_delta_pattern(pm, regions, gems_delta: int, vault_delta: int,
                         gold_delta: int = 0) -> list[tuple[int, dict]]:
    """Search for InventoryDelta objects matching known delta values.

    InventoryDelta (IL2CPP class) has these value-type fields:
        gemsDelta: int
        goldDelta: int
        vaultProgressDelta: decimal (likely f64 or i32 in IL2CPP)
        wcTrackPosition: int
        wcCommonDelta: int
        wcUncommonDelta: int
        wcRareDelta: int
        wcMythicDelta: int
    Plus reference fields (pointers):
        boosterDelta: BoosterStack[]
        cardsAdded: int[]
        decksAdded: Guid[]
        vanityItemsAdded/Removed: string[]
        artSkinsAdded/Removed: ArtSkin[]

    IL2CPP layout: reference fields first, then value fields.
    So the pointers come before the int deltas.

    Strategy: Search for gemsDelta, then check if goldDelta is nearby.
    """
    results = []

    # Search for the most distinctive delta value
    # If gems_delta is 0, use vault_delta instead
    search_val = gems_delta if gems_delta != 0 else vault_delta
    if search_val == 0:
        print("  WARNING: Both gems and vault deltas are 0. No distinctive anchor.")
        return results

    print(f"  Searching for i32={search_val} (gems_delta={gems_delta}, gold_delta={gold_delta})...")
    hits = global_search_i32(pm, regions, search_val)
    print(f"  Found {len(hits)} raw hits")

    # For each hit, check surrounding memory for the delta pattern
    for addr in hits:
        # Read 128 bytes around the hit (64 before, 64 after)
        context_start = addr - 64
        data = read_memory(pm, context_start, 128)
        if data is None:
            continue

        # The hit is at offset 64 in our read
        hit_offset = 64

        # Score this candidate by checking for other expected delta values nearby
        score = 0
        details = {"address": addr, "search_val": search_val}

        # Check all 4-byte-aligned positions within ±32 bytes for gold_delta
        for check_off in range(max(0, hit_offset - 32), min(len(data) - 3, hit_offset + 32), 4):
            val = struct.unpack_from("<i", data, check_off)[0]
            if val == gold_delta and check_off != hit_offset:
                rel = check_off - hit_offset
                score += 1
                details[f"gold_delta_at_rel_{rel}"] = val

        # Check for vault delta (might be stored as i32 or different scaling)
        for check_off in range(max(0, hit_offset - 32), min(len(data) - 3, hit_offset + 32), 4):
            val = struct.unpack_from("<i", data, check_off)[0]
            if val == vault_delta and vault_delta != 0 and check_off != hit_offset:
                rel = check_off - hit_offset
                score += 1
                details[f"vault_delta_at_rel_{rel}"] = val

        # Check for IL2CPP object header: 8-byte-aligned pointer at some offset before the hit
        # Look for klass pointer pattern
        for check_off in range(0, hit_offset, 8):
            ptr = struct.unpack_from("<Q", data, check_off)[0]
            if 0x1_0000_0000 <= ptr <= 0x7FFF_FFFF_FFFF:
                # Check if next 8 bytes are null (monitor)
                if check_off + 8 < len(data):
                    monitor = struct.unpack_from("<Q", data, check_off + 8)[0]
                    if monitor == 0:
                        distance = hit_offset - check_off
                        if 16 <= distance <= 128:
                            score += 1
                            details["possible_obj_start"] = context_start + check_off
                            details["klass"] = ptr
                            details["value_offset_from_obj"] = distance
                            break

        if score >= 1:
            # Dump the full context for analysis
            details["score"] = score
            i32_vals = []
            for i in range(0, len(data), 4):
                i32_vals.append(struct.unpack_from("<i", data, i)[0])
            details["context_i32s"] = i32_vals
            details["context_start"] = context_start
            results.append((addr, details))

    return results


def search_booster_delta(pm, regions, collation_id: int) -> list[tuple[int, dict]]:
    """Search for BoosterStack objects with a specific CollationId and Count=-1 (delta).

    BoosterStack layout from field-offset-map:
        +0: klass (8 bytes)
        +8: monitor (null, 8 bytes)
        +16: CollationId (i32)
        +20: Count (i32)

    For a delta BoosterStack, Count should be -1 (one pack opened).
    """
    results = []

    print(f"  Searching for CollationId={collation_id}...")
    hits = global_search_i32(pm, regions, collation_id)
    print(f"  Found {len(hits)} raw hits for {collation_id}")

    for addr in hits:
        # Read context: 24 bytes before (for klass+monitor) and 16 after
        data = read_memory(pm, addr - 16, 40)
        if data is None:
            continue

        # If this is a BoosterStack at +16, the CollationId is at our addr
        # which means klass is at addr-16, monitor at addr-8, CollationId at addr, Count at addr+4
        klass = struct.unpack_from("<Q", data, 0)[0]
        monitor = struct.unpack_from("<Q", data, 8)[0]
        coll_id = struct.unpack_from("<i", data, 16)[0]
        count = struct.unpack_from("<i", data, 20)[0]
        field_24 = struct.unpack_from("<i", data, 24)[0]

        is_obj = (0x1_0000_0000 <= klass <= 0x7FFF_FFFF_FFFF) and monitor == 0
        is_delta = count < 0  # delta would be negative

        if is_obj:
            details = {
                "address": addr - 16,  # object start
                "klass": klass,
                "collation_id": coll_id,
                "count": count,
                "field_24": field_24,
                "is_delta": is_delta,
            }
            results.append((addr, details))

    return results


def hunt_report(pm, regions, inv_addr: int,
                before: dict[str, int], after: dict[str, int]) -> None:
    """Main report hunt: search for delta patterns after inventory change."""
    print("\n" + "=" * 60)
    print("REPORT QUEUE HUNT")
    print("=" * 60)

    # Compute deltas
    deltas = {}
    for key in before:
        deltas[key] = after[key] - before[key]

    print("\nDetected deltas:")
    for key, delta in deltas.items():
        if delta != 0:
            print(f"  {key:15s}: {before[key]:>8} -> {after[key]:>8}  (delta: {delta:+d})")
        else:
            print(f"  {key:15s}: {before[key]:>8}  (unchanged)")

    gems_delta = deltas.get("gems", 0)
    gold_delta = deltas.get("gold", 0)
    vault_delta = deltas.get("vault_raw", 0)

    # Strategy 1: Search for InventoryDelta by gems/gold delta pattern
    print(f"\n--- Strategy 1: Search for InventoryDelta ---")
    delta_results = search_delta_pattern(pm, regions, gems_delta, vault_delta, gold_delta)
    print(f"  {len(delta_results)} scored candidates")

    # Sort by score
    delta_results.sort(key=lambda x: x[1].get("score", 0), reverse=True)

    for addr, details in delta_results[:10]:
        score = details.get("score", 0)
        print(f"\n  Candidate at {addr:#x} (score={score}):")
        for k, v in details.items():
            if k == "context_i32s":
                # Print as rows of 4
                vals = v
                for row_start in range(0, len(vals), 4):
                    row = vals[row_start:row_start + 4]
                    ctx_addr = details["context_start"] + row_start * 4
                    print(f"    {ctx_addr:#x}: {row}")
            elif k not in ("context_start",):
                if isinstance(v, int) and v > 0xFFFF:
                    print(f"    {k}: {v:#x}")
                else:
                    print(f"    {k}: {v}")

    # Strategy 2: Search for booster delta objects
    print(f"\n--- Strategy 2: Search for BoosterStack deltas ---")
    # Search for CollationId 100032 (ONE) with negative count
    booster_results = search_booster_delta(pm, regions, 100032)
    delta_boosters = [r for r in booster_results if r[1]["is_delta"]]
    positive_boosters = [r for r in booster_results if not r[1]["is_delta"]]
    print(f"  {len(booster_results)} total, {len(delta_boosters)} with negative count, "
          f"{len(positive_boosters)} with positive/zero count")

    for addr, details in delta_boosters[:10]:
        print(f"\n  DELTA booster at {details['address']:#x}:")
        print(f"    klass: {details['klass']:#x}")
        print(f"    CollationId: {details['collation_id']}")
        print(f"    Count: {details['count']}")
        print(f"    field_24: {details['field_24']}")

        # Read more context around this object
        extended = read_memory(pm, details["address"] - 32, 128)
        if extended:
            print(f"    Extended context (obj-32 to obj+96):")
            for i in range(0, len(extended), 8):
                val_u64 = struct.unpack_from("<Q", extended, i)[0]
                val_i32_a = struct.unpack_from("<i", extended, i)[0]
                val_i32_b = struct.unpack_from("<i", extended, i + 4)[0]
                ctx_addr = details["address"] - 32 + i
                if 0x1_0000_0000 <= val_u64 <= 0x7FFF_FFFF_FFFF:
                    print(f"      {ctx_addr:#x}: PTR {val_u64:#x}")
                else:
                    print(f"      {ctx_addr:#x}: {val_i32_a}, {val_i32_b}")

    # Strategy 3: Search for wc track delta if it changed
    wc_track_delta = deltas.get("wc_track", 0)
    if wc_track_delta != 0:
        print(f"\n--- Strategy 3: WC track delta ({wc_track_delta:+d}) ---")
        wc_hits = global_search_i32(pm, regions, wc_track_delta)
        print(f"  {len(wc_hits)} raw hits for wc_track_delta={wc_track_delta}")
        # Could narrow further but likely too many hits for small values

    # Strategy 4: Search for specific wildcard deltas
    for wc_name in ["wc_common", "wc_uncommon", "wc_rare", "wc_mythic"]:
        wc_delta = deltas.get(wc_name, 0)
        if wc_delta != 0:
            print(f"\n--- Strategy 4: {wc_name} delta ({wc_delta:+d}) ---")
            # This is interesting because WC deltas from pack opens are rare
            print(f"  (Wildcard count changed — unusual for pack open, may indicate WC track completion)")


def main() -> int:
    parser = argparse.ArgumentParser(description="Hunt for report queue objects")
    parser.add_argument("--mode", choices=["watch", "search"], default="watch",
                        help="watch=poll for changes; search=immediate search with known deltas")
    parser.add_argument("--gold", type=int, default=None, help="Override gold for inventory search")
    parser.add_argument("--gems-delta", type=int, default=None, help="Known gems delta (for search mode)")
    parser.add_argument("--vault-delta", type=int, default=None, help="Known vault delta (for search mode)")
    args = parser.parse_args()

    sh = extract_starthook()
    overrides = {}
    if args.gold is not None:
        overrides["gold"] = args.gold

    known = {
        "gems": sh.gems,
        "wc_common": sh.wc_common,
        "wc_uncommon": sh.wc_uncommon,
        "wc_rare": sh.wc_rare,
        "wc_track_position": sh.wc_track_position,
    }
    if overrides.get("gold"):
        known["gold"] = overrides["gold"]
    known = {k: v for k, v in known.items() if v != 0}

    print(f"Opening MTGA.exe...")
    pm = open_mtga()
    print(f"  PID: {pm.process_id}")

    print(f"Enumerating memory regions...")
    regions = enumerate_regions(pm)
    print(f"  {len(regions)} readable regions")

    print(f"Searching for inventory ({len(known)} anchor values)...")
    location = find_inventory(pm, regions, known)
    if location is None:
        print("ERROR: Inventory not found", file=sys.stderr)
        pm.close_process()
        return 1

    inv_addr = location.address
    print(f"  Inventory at {inv_addr:#x}")

    if args.mode == "watch":
        print("\n" + "=" * 60)
        print("WATCHING for inventory changes...")
        print("Open a pack, buy something, or claim a reward NOW.")
        print("Press Ctrl+C to stop.")
        print("=" * 60)

        before = read_inventory_scalars(pm, inv_addr)
        if before is None:
            print("ERROR: Failed to read inventory", file=sys.stderr)
            pm.close_process()
            return 1

        print(f"\nCurrent state: gold={before['gold']}, gems={before['gems']}, "
              f"vault={before['vault_raw']}")

        poll_count = 0
        try:
            while True:
                time.sleep(0.05)  # 50ms polling
                poll_count += 1
                current = read_inventory_scalars(pm, inv_addr)
                if current is None:
                    continue

                if current != before:
                    print(f"\n*** CHANGE DETECTED after {poll_count} polls "
                          f"({poll_count * 0.05:.1f}s) ***")

                    # Re-enumerate regions (new objects may be in new regions)
                    regions = enumerate_regions(pm)

                    hunt_report(pm, regions, inv_addr, before, current)

                    # Update before for next change
                    before = current
                    print("\n\nContinuing to watch... (Ctrl+C to stop)")

        except KeyboardInterrupt:
            print("\nStopped.")

    else:
        # Search mode with known deltas
        after = read_inventory_scalars(pm, inv_addr)
        before_vals = {
            "gold": sh.gold if args.gold is None else args.gold,
            "gems": sh.gems,
            "wc_common": sh.wc_common,
            "wc_uncommon": sh.wc_uncommon,
            "wc_rare": sh.wc_rare,
            "wc_mythic": sh.wc_mythic,
            "wc_track": sh.wc_track_position,
            "vault_raw": sh.vault_progress_raw,
        }
        if after:
            hunt_report(pm, regions, inv_addr, before_vals, after)

    pm.close_process()
    return 0


if __name__ == "__main__":
    sys.exit(main())
