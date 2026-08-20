"""Verify previously discovered memory targets are still valid.

Reads known addresses from prior sessions and validates field values.
Run from mtga-tool/: python -m python.research.verify
"""
from __future__ import annotations

import struct
import sys

from .probe import open_mtga, read_memory, read_i32, read_i64, enumerate_regions, find_inventory
from .starthook import extract_starthook


def read_il2cpp_string(pm, addr: int) -> str | None:
    """Read an IL2CPP String object: klass(8) + monitor(8) + length(4) + chars(UTF-16LE)."""
    header = read_memory(pm, addr, 20)
    if header is None:
        return None
    length = struct.unpack_from("<i", header, 16)[0]
    if length <= 0 or length > 500:
        return None
    char_data = read_memory(pm, addr + 20, length * 2)
    if char_data is None:
        return None
    try:
        return char_data.decode("utf-16-le")
    except Exception:
        return None


def verify_cosmetics(pm, inv_addr: int, sh) -> bool:
    """Verify CosmeticsClient by re-discovering via 6-list pattern match."""
    print("\n=== CosmeticsClient Verification ===")

    expected_sizes = [sh.art_styles_count, sh.avatars_count, sh.pets_count,
                      sh.sleeves_count, sh.emotes_count, sh.titles_count]
    print(f"  Expected list sizes from StartHook: {expected_sizes}")

    # Re-discover: scan heap for object with 6 consecutive list pointers matching sizes
    # Use global search since it's NOT reachable from inventory
    regions = enumerate_regions(pm)
    chunk_size = 4 * 1024 * 1024
    candidates = []

    print(f"  Scanning {len(regions)} regions for CosmeticsClient pattern...")

    for region in regions:
        offset = 0
        while offset < region.size:
            read_size = min(chunk_size, region.size - offset)
            if read_size < 64:
                offset += read_size
                continue
            try:
                data = pm.read_bytes(region.base + offset, read_size)
            except Exception:
                offset += read_size
                continue

            # Scan for: klass(8) + null(8) + 6 pointers(48) = 64 bytes
            for i in range(0, len(data) - 63, 8):
                klass = struct.unpack_from("<Q", data, i)[0]
                monitor = struct.unpack_from("<Q", data, i + 8)[0]

                if monitor != 0:
                    continue
                if not (0x1_0000_0000 <= klass <= 0x7FFF_FFFF_FFFF):
                    continue

                # Check if next 6 slots are valid pointers
                ptrs = []
                all_valid = True
                for j in range(6):
                    ptr = struct.unpack_from("<Q", data, i + 16 + j * 8)[0]
                    if not (0x1_0000_0000 <= ptr <= 0x7FFF_FFFF_FFFF):
                        all_valid = False
                        break
                    ptrs.append(ptr)

                if not all_valid:
                    continue

                # Check first list _size matches expected
                first_target = read_memory(pm, ptrs[0], 32)
                if first_target is None:
                    continue
                first_size = struct.unpack_from("<i", first_target, 24)[0]
                if first_size != expected_sizes[0]:
                    continue

                # Check remaining 5 list sizes
                all_match = True
                sizes = [first_size]
                for j in range(1, 6):
                    target = read_memory(pm, ptrs[j], 32)
                    if target is None:
                        all_match = False
                        break
                    s = struct.unpack_from("<i", target, 24)[0]
                    sizes.append(s)
                    if s != expected_sizes[j]:
                        all_match = False
                        break

                if all_match:
                    obj_addr = region.base + offset + i
                    candidates.append((obj_addr, klass, sizes))

            offset += read_size

    if not candidates:
        print("  FAIL: CosmeticsClient not found")
        return False

    print(f"  Found {len(candidates)} candidate(s)")
    for addr, klass, sizes in candidates:
        print(f"  Address: {addr:#x}, klass: {klass:#x}")
        labels = ["ArtStyles", "Avatars", "Pets", "Sleeves", "Emotes", "Titles"]
        for label, size, expected in zip(labels, sizes, expected_sizes):
            status = "MATCH" if size == expected else "MISMATCH"
            print(f"    {label:12s}: {size:3d} (expected {expected}) {status}")

    return len(candidates) == 1


def verify_rank(pm, inv_addr: int, sh) -> bool:
    """Verify Rank State by re-discovering via season_ordinal anchor search."""
    print("\n=== Rank State Verification ===")

    season = sh.season_ordinal
    print(f"  Anchoring on season_ordinal={season}")

    # Global search for season_ordinal
    regions = enumerate_regions(pm)
    chunk_size = 4 * 1024 * 1024
    season_bytes = struct.pack("<i", season)

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
                idx = data.find(season_bytes, pos)
                if idx == -1:
                    break
                hits.append(region.base + offset + idx)
                pos = idx + 1

            offset += read_size

    print(f"  Found {len(hits)} raw hits for season_ordinal={season}")

    # Score each hit: check for double season_ordinal pattern and matching rank fields
    scored = []
    for addr in hits:
        # Read 68 bytes: the full rank struct starting from con_season_ordinal
        data = read_memory(pm, addr, 52)
        if data is None:
            continue

        vals = [struct.unpack_from("<i", data, i)[0] for i in range(0, 52, 4)]
        # Expected layout: con_season, con_unk, con_level, con_step, con_wins, con_draws, con_losses,
        #                   ltd_season, ltd_unk, ltd_level, ltd_step, ltd_wins, ltd_losses

        score = 0
        if len(vals) >= 13:
            if vals[0] == season:
                score += 1  # con_season
            if vals[7] == season:
                score += 1  # ltd_season (at +28)
            if vals[2] == sh.constructed_level:
                score += 1
            if vals[3] == sh.constructed_step:
                score += 1
            if vals[4] == sh.constructed_wins:
                score += 1
            if vals[9] == sh.limited_level:
                score += 1
            if vals[10] == sh.limited_step:
                score += 1
            if vals[11] == sh.limited_wins:
                score += 1

            # Check klass at -16
            klass_data = read_memory(pm, addr - 16, 16)
            has_klass = False
            if klass_data:
                klass = struct.unpack_from("<Q", klass_data, 0)[0]
                monitor = struct.unpack_from("<Q", klass_data, 8)[0]
                if (0x1_0000_0000 <= klass <= 0x7FFF_FFFF_FFFF) and monitor == 0:
                    has_klass = True

            if score >= 7 and has_klass:
                scored.append((addr, score, vals, klass))

    print(f"  {len(scored)} candidate(s) with score >= 7 and valid klass")

    if not scored:
        print("  FAIL: Rank state not found")
        return False

    for addr, score, vals, klass in scored:
        print(f"  Address: {addr:#x} (obj at {addr - 16:#x}), score={score}/8, klass={klass:#x}")
        fields = [
            ("con_season", vals[0], season),
            ("con_unknown", vals[1], None),
            ("con_level", vals[2], sh.constructed_level),
            ("con_step", vals[3], sh.constructed_step),
            ("con_wins", vals[4], sh.constructed_wins),
            ("con_draws", vals[5], None),
            ("con_losses", vals[6], None),
            ("ltd_season", vals[7], season),
            ("ltd_unknown", vals[8], None),
            ("ltd_level", vals[9], sh.limited_level),
            ("ltd_step", vals[10], sh.limited_step),
            ("ltd_wins", vals[11], sh.limited_wins),
            ("ltd_losses", vals[12], sh.limited_losses),
        ]
        for name, mem_val, exp_val in fields:
            if exp_val is not None:
                status = "MATCH" if mem_val == exp_val else "MISMATCH"
                print(f"    {name:16s}: {mem_val:5d} (expected {exp_val}) {status}")
            else:
                print(f"    {name:16s}: {mem_val:5d} (no StartHook ref)")

    return len(scored) >= 1


def verify_mastery(pm, inv_addr: int, sh) -> bool:
    """Verify Mastery LevelTrack by searching for current_progress anchor."""
    print("\n=== Mastery LevelTrack Verification ===")

    # We need the current mastery progress. Check if it changed since last session.
    # Last known: level 24, progress 575
    # Try searching for 575 first, then fall back to broad search
    progress = 575  # from last session's GraphGetGraphState
    print(f"  Last known progress: {progress} XP (level 24)")
    print(f"  NOTE: Progress may have changed since last check. Searching for {progress}...")

    regions = enumerate_regions(pm)
    chunk_size = 4 * 1024 * 1024
    progress_bytes = struct.pack("<i", progress)

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
                idx = data.find(progress_bytes, pos)
                if idx == -1:
                    break
                hits.append(region.base + offset + idx)
                pos = idx + 1

            offset += read_size

    print(f"  Found {len(hits)} raw hits for i32={progress}")

    # Validate each hit using the LevelTrack criteria
    candidates = []
    for addr in hits:
        # current_progress is at element_base + 48
        element_base = addr - 48

        # Read the full 64-byte element + previous sibling
        data = read_memory(pm, element_base - 64, 128)
        if data is None:
            continue

        # Current element starts at offset 64 in our read
        off = 64

        klass = struct.unpack_from("<Q", data, off)[0]
        monitor = struct.unpack_from("<Q", data, off + 8)[0]
        ptr_a = struct.unpack_from("<Q", data, off + 16)[0]
        ptr_c = struct.unpack_from("<Q", data, off + 32)[0]
        prev_level = struct.unpack_from("<i", data, off + 40)[0]
        level_num = struct.unpack_from("<i", data, off + 44)[0]
        curr_progress = struct.unpack_from("<i", data, off + 48)[0]

        score = 0

        # Basic validity
        if not (0x1_0000_0000 <= klass <= 0x7FFF_FFFF_FFFF):
            continue
        if monitor != 0:
            continue
        if not (0x1_0000_0000 <= ptr_a <= 0x7FFF_FFFF_FFFF):
            continue
        if not (0x1_0000_0000 <= ptr_c <= 0x7FFF_FFFF_FFFF):
            continue
        if not (1 <= level_num <= 200):
            continue
        if prev_level != level_num - 1:
            continue

        # Sibling klass match
        sib_klass = struct.unpack_from("<Q", data, 0)[0]
        if sib_klass == klass:
            score += 2

        # XP requirement cross-check
        xp_data = read_memory(pm, ptr_a, 24)
        if xp_data:
            xp_val = struct.unpack_from("<i", xp_data, 16)[0]
            if xp_val == 1000:
                score += 2

        if score >= 4:
            # Read the name string
            name = read_il2cpp_string(pm, ptr_c)
            candidates.append((element_base, level_num, curr_progress, klass, name, score))

    print(f"  {len(candidates)} validated candidate(s) (score >= 4)")

    if not candidates:
        print("  FAIL: LevelTrack nodes not found")
        print("  Progress may have changed. Try a different anchor value.")
        return False

    for base, level, prog, klass, name, score in candidates:
        print(f"  Address: {base:#x}, level={level}, progress={prog}, klass={klass:#x}")
        print(f"    Name: {name}")
        print(f"    Score: {score}/4")

        # Read a few adjacent levels
        for delta in [-1, 1]:
            adj_addr = base + delta * 64
            adj_data = read_memory(pm, adj_addr, 64)
            if adj_data:
                adj_klass = struct.unpack_from("<Q", adj_data, 0)[0]
                adj_level = struct.unpack_from("<i", adj_data, 44)[0]
                adj_prog = struct.unpack_from("<i", adj_data, 48)[0]
                if adj_klass == klass:
                    direction = "prev" if delta == -1 else "next"
                    print(f"    Adjacent ({direction}): level={adj_level}, progress={adj_prog}")

    return True


def main() -> int:
    print("Memory Target Verification")
    print("=" * 60)

    sh = extract_starthook()
    print(f"Opening MTGA.exe...")
    pm = open_mtga()
    print(f"  PID: {pm.process_id}")

    # Find inventory with gold override
    known = {
        "gems": sh.gems,
        "wc_common": sh.wc_common,
        "wc_uncommon": sh.wc_uncommon,
        "wc_rare": sh.wc_rare,
        "wc_track_position": sh.wc_track_position,
    }
    known = {k: v for k, v in known.items() if v != 0}
    print(f"Searching for inventory ({len(known)} anchor values, excluding gold)...")

    regions = enumerate_regions(pm)
    location = find_inventory(pm, regions, known)
    if location is None:
        print("ERROR: Inventory not found")
        pm.close_process()
        return 1

    inv_addr = location.address
    print(f"  Inventory at {inv_addr:#x}")

    results = {}

    # Verify all three targets
    results["cosmetics"] = verify_cosmetics(pm, inv_addr, sh)
    results["rank"] = verify_rank(pm, inv_addr, sh)
    results["mastery"] = verify_mastery(pm, inv_addr, sh)

    print("\n" + "=" * 60)
    print("Verification Summary")
    print("=" * 60)
    for target, passed in results.items():
        status = "PASS" if passed else "FAIL"
        print(f"  {target:20s}: {status}")

    pm.close_process()
    return 0


if __name__ == "__main__":
    sys.exit(main())
