"""Session 7 automated recon — check report queue and rank duplicates.

No in-game actions required. Reads existing state only.
Run from mtga-tool/: python -m python.research.session7_recon
"""
from __future__ import annotations

import struct
import sys

from .probe import (
    enumerate_regions,
    find_inventory,
    open_mtga,
    read_i32,
    read_i64,
    read_memory,
)
from .starthook import extract_starthook
from .verify import read_il2cpp_string
from .delta_mapper import (
    find_inventory_client,
    read_queue_state,
    read_report_item,
    read_context_source_type,
    dump_inventory_delta,
    read_aetherized_cards,
    SOURCE_NAMES,
)


def check_report_queue(pm, regions, inv_addr: int) -> None:
    """Check the report queue status and dump any existing items."""
    print("\n" + "=" * 60)
    print("REPORT QUEUE STATUS")
    print("=" * 60)

    inv_obj_base = inv_addr - 80
    print(f"  Inventory object base: {inv_obj_base:#x}")

    # Find InventoryClient
    print(f"  Searching for InventoryClient...")
    inv_client = find_inventory_client(pm, regions, inv_obj_base)
    if inv_client is None:
        print("  ERROR: InventoryClient not found.")
        return

    # Read queue list pointer
    queue_list_ptr = read_i64(pm, inv_client + 112)
    if queue_list_ptr is None or queue_list_ptr == 0:
        print("  ERROR: Queue list pointer at +112 is null.")
        return

    print(f"  Queue List at {queue_list_ptr:#x}")

    # Read queue state
    state = read_queue_state(pm, queue_list_ptr)
    if state is None:
        print("  ERROR: Failed to read queue state.")
        return

    items_ptr, q_size, q_version = state
    print(f"  Queue: _size={q_size}, _version={q_version}, _items={items_ptr:#x}")

    if q_size == 0:
        print("  Queue is empty (no pending report items).")
        return

    print(f"\n  Dumping {q_size} report item(s)...")

    for i in range(q_size):
        elem_ptr = read_i64(pm, items_ptr + 32 + i * 8)
        if elem_ptr is None or elem_ptr == 0:
            print(f"\n  [{i}] null element pointer")
            continue

        item = read_report_item(pm, elem_ptr)
        if item is None:
            print(f"\n  [{i}] failed to read at {elem_ptr:#x}")
            continue

        source_type = read_context_source_type(pm, item["context_ptr"])
        source_name = SOURCE_NAMES.get(source_type, f"unknown({source_type})")

        print(f"\n  [{i}] ReportItem at {elem_ptr:#x}")
        print(f"      Source: {source_name} ({source_type})")
        print(f"      XP gained: {item['xp_gained']}")

        # Parent context
        if item["parent_ptr"] and (0x1_0000_0000 <= item["parent_ptr"] <= 0x7FFF_FFFF_FFFF):
            parent_str = read_il2cpp_string(pm, item["parent_ptr"])
            print(f"      Parent context: {parent_str}")

        # Quick delta summary
        delta = dump_inventory_delta(pm, item["delta_ptr"])
        if delta:
            # Show non-zero value fields
            non_zero = {off: val for off, val in delta["value_i32s"].items() if val != 0 and off < 168}
            if non_zero:
                print(f"      Non-zero delta values: {non_zero}")
            else:
                print(f"      Delta values: all zeros in mapped range (+104 to +168)")

            # Check boosterDelta ref
            booster_delta_ptr = delta["ref_ptrs"][0]  # +16 = boosterDelta
            if booster_delta_ptr and (0x1_0000_0000 <= booster_delta_ptr <= 0x7FFF_FFFF_FFFF):
                arr_data = read_memory(pm, booster_delta_ptr, 40)
                if arr_data:
                    max_len = struct.unpack_from("<Q", arr_data, 24)[0]
                    if max_len < 100:
                        print(f"      BoosterDelta: array with {max_len} element(s)")

        # Aetherized card count
        if item["aetherized_ptr"] and (0x1_0000_0000 <= item["aetherized_ptr"] <= 0x7FFF_FFFF_FFFF):
            list_data = read_memory(pm, item["aetherized_ptr"], 32)
            if list_data:
                card_count = struct.unpack_from("<i", list_data, 24)[0]
                print(f"      Aetherized cards: {card_count}")


def investigate_rank_duplicates(pm, regions, sh) -> None:
    """Find and compare all rank state candidates to understand the duplicate."""
    print("\n" + "=" * 60)
    print("RANK STATE DUPLICATE INVESTIGATION")
    print("=" * 60)

    season = sh.season_ordinal
    season_bytes = struct.pack("<i", season)
    chunk_size = 4 * 1024 * 1024

    # Find all season_ordinal hits with high score
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

    # Score each hit
    scored = []
    for addr in hits:
        data = read_memory(pm, addr, 52)
        if data is None:
            continue
        vals = [struct.unpack_from("<i", data, i)[0] for i in range(0, 52, 4)]
        if len(vals) < 13:
            continue

        score = 0
        if vals[0] == season:
            score += 1
        if vals[7] == season:
            score += 1
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

        klass_data = read_memory(pm, addr - 16, 16)
        klass = None
        if klass_data:
            klass = struct.unpack_from("<Q", klass_data, 0)[0]
            monitor = struct.unpack_from("<Q", klass_data, 8)[0]
            if not (0x1_0000_0000 <= klass <= 0x7FFF_FFFF_FFFF) or monitor != 0:
                klass = None

        if score >= 7 and klass is not None:
            scored.append((addr, score, vals, klass))

    print(f"  Found {len(scored)} high-score candidates (>= 7/8)")

    for i, (addr, score, vals, klass) in enumerate(scored):
        obj_addr = addr - 16
        print(f"\n  Candidate {i+1}: obj at {obj_addr:#x}, klass={klass:#x}, score={score}/8")

        # Read extended context around the object (128 bytes before, 128 after)
        before_data = read_memory(pm, obj_addr - 64, 64)
        after_data = read_memory(pm, obj_addr + 68, 64)  # 68 = 16 header + 52 scalars

        # What's BEFORE this object? (previous heap object's data)
        if before_data:
            prev_ptrs = []
            for j in range(0, 64, 8):
                pval = struct.unpack_from("<Q", before_data, j)[0]
                if 0x1_0000_0000 <= pval <= 0x7FFF_FFFF_FFFF:
                    prev_ptrs.append((j - 64, pval))
            if prev_ptrs:
                print(f"    Before object (pointers): {[(off, f'{p:#x}') for off, p in prev_ptrs[:4]]}")

        # What's AFTER this object?
        if after_data:
            # Check if there's another klass pointer right after (next GC object)
            next_klass = struct.unpack_from("<Q", after_data, 0)[0]
            next_monitor = struct.unpack_from("<Q", after_data, 8)[0]
            if (0x1_0000_0000 <= next_klass <= 0x7FFF_FFFF_FFFF) and next_monitor == 0:
                print(f"    Next object at {obj_addr + 68:#x}, klass={next_klass:#x}")
            else:
                # Check as i32 values
                after_vals = [struct.unpack_from("<i", after_data, j)[0] for j in range(0, min(32, len(after_data)), 4)]
                print(f"    After data: {after_vals}")

        # Search for parent pointers to this object
        obj_bytes = struct.pack("<Q", obj_addr)
        parent_count = 0
        parent_addrs = []
        for region in regions:
            off = 0
            while off < region.size:
                rs = min(chunk_size, region.size - off)
                try:
                    rdata = pm.read_bytes(region.base + off, rs)
                except Exception:
                    off += rs
                    continue
                p = 0
                while True:
                    idx = rdata.find(obj_bytes, p)
                    if idx == -1:
                        break
                    parent_addrs.append(region.base + off + idx)
                    parent_count += 1
                    p = idx + 1
                off += rs

        print(f"    Parent pointers to this object: {parent_count}")
        for pa in parent_addrs[:5]:
            # Read context around parent pointer
            pctx = read_memory(pm, pa - 16, 32)
            if pctx:
                ctx_klass = struct.unpack_from("<Q", pctx, 0)[0]
                ctx_ptr_off = pa % 8
                # Try to find what object this parent pointer belongs to
                # Check if the pointer at pa is part of a larger object by looking for klass before it
                for check_off in range(0, 256, 8):
                    check_addr = pa - check_off
                    check_data = read_memory(pm, check_addr, 16)
                    if check_data:
                        ck = struct.unpack_from("<Q", check_data, 0)[0]
                        cm = struct.unpack_from("<Q", check_data, 8)[0]
                        if (0x1_0000_0000 <= ck <= 0x7FFF_FFFF_FFFF) and cm == 0 and check_off > 0:
                            print(f"      Parent at {pa:#x} (offset +{check_off} in object at {check_addr:#x}, klass={ck:#x})")
                            break
                else:
                    print(f"      Parent at {pa:#x}")


def check_cosmetics_address_stability(pm, regions, sh) -> None:
    """Check if CosmeticsClient address matches previous session."""
    print("\n" + "=" * 60)
    print("COSMETICS ADDRESS STABILITY CHECK")
    print("=" * 60)

    # Previous session address: 0x1d154938cc0
    # This session (from verify output): 0x1d15daa2b80
    prev_addr = 0x1d154938cc0
    curr_addr = 0x1d15daa2b80

    print(f"  Previous session: {prev_addr:#x}")
    print(f"  Current session:  {curr_addr:#x}")
    print(f"  Offset:           {curr_addr - prev_addr:+d} bytes ({(curr_addr - prev_addr) / 1024 / 1024:+.1f} MB)")

    # Read both and check validity
    for label, addr in [("Previous", prev_addr), ("Current", curr_addr)]:
        data = read_memory(pm, addr, 64)
        if data is None:
            print(f"  {label}: UNREADABLE (GC reclaimed)")
            continue
        klass = struct.unpack_from("<Q", data, 0)[0]
        monitor = struct.unpack_from("<Q", data, 8)[0]
        if (0x1_0000_0000 <= klass <= 0x7FFF_FFFF_FFFF) and monitor == 0:
            # Check first list
            first_ptr = struct.unpack_from("<Q", data, 16)[0]
            if 0x1_0000_0000 <= first_ptr <= 0x7FFF_FFFF_FFFF:
                list_data = read_memory(pm, first_ptr, 32)
                if list_data:
                    list_size = struct.unpack_from("<i", list_data, 24)[0]
                    print(f"  {label}: klass={klass:#x}, first_list_size={list_size}")
                    continue
        print(f"  {label}: data at address but not valid CosmeticsClient")


def main() -> int:
    print("Session 7 Automated Recon")
    print("=" * 60)

    sh = extract_starthook()
    print(f"Opening MTGA.exe...")
    pm = open_mtga()
    print(f"  PID: {pm.process_id}")

    regions = enumerate_regions(pm)
    print(f"  {len(regions)} readable regions")

    # Find inventory
    known = {
        "gems": sh.gems,
        "wc_common": sh.wc_common,
        "wc_uncommon": sh.wc_uncommon,
        "wc_rare": sh.wc_rare,
        "wc_mythic": sh.wc_mythic,
        "wc_track_position": sh.wc_track_position,
        "gold": sh.gold,
    }
    known = {k: v for k, v in known.items() if v != 0}

    print(f"  Finding inventory ({len(known)} anchors)...")
    location = find_inventory(pm, regions, known)
    if location is None:
        print("ERROR: Inventory not found")
        pm.close_process()
        return 1

    inv_addr = location.address
    print(f"  Inventory at {inv_addr:#x} (score={location.score})")

    # 1. Check report queue
    check_report_queue(pm, regions, inv_addr)

    # 2. Investigate rank duplicates
    investigate_rank_duplicates(pm, regions, sh)

    # 3. Check cosmetics address stability
    check_cosmetics_address_stability(pm, regions, sh)

    pm.close_process()
    print("\n\nRecon complete.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
