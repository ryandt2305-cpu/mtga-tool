"""Burst capture — polls inventory, captures full memory snapshot on change.

Captures ALL pointers to the delta booster region and full object graphs
within 50ms of an inventory change.

Run: python -m python.research.burst_capture [--gold 800]
Then perform an in-game action (open pack, buy something, claim reward).
"""
from __future__ import annotations

import argparse
import json
import struct
import sys
import time
from pathlib import Path

from .probe import (
    enumerate_regions,
    find_inventory,
    open_mtga,
    read_i32,
    read_i64,
    read_memory,
)
from .starthook import extract_starthook


def read_scalars(pm, addr: int) -> dict[str, int] | None:
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
        "vault_raw": int(struct.unpack_from("<d", data, 32)[0] * 10),
    }


def read_string(pm, addr: int) -> str | None:
    if addr == 0 or not (0x1_0000_0000 <= addr <= 0x7FFF_FFFF_FFFF):
        return None
    hdr = read_memory(pm, addr, 24)
    if hdr is None:
        return None
    length = struct.unpack_from("<i", hdr, 16)[0]
    if length <= 0 or length > 200:
        return None
    chars = read_memory(pm, addr + 20, length * 2)
    if chars is None:
        return None
    try:
        return chars.decode("utf-16-le")
    except Exception:
        return None


def burst_capture(pm, regions, inv_addr: int,
                  before: dict, after: dict, timestamp: float) -> dict:
    """Capture everything reachable from inventory and new objects."""
    result = {
        "timestamp": timestamp,
        "before": before,
        "after": after,
        "deltas": {k: after[k] - before[k] for k in before},
    }

    # 1. Read inventory reference fields (inv-80 to inv-8)
    inv_refs = {}
    ref_data = read_memory(pm, inv_addr - 80, 80)
    if ref_data:
        labels = [
            (-80, "klass"), (-72, "monitor"), (-64, "boosters"),
            (-56, "vouchers"), (-48, "prizeWallsUnlocked"),
            (-40, "basicLandSet"), (-32, "latestBasicLandSet"),
            (-24, "starterDecks"), (-16, "tickets"), (-8, "customTokens"),
        ]
        for off, name in labels:
            real_off = off + 80
            ptr = struct.unpack_from("<Q", ref_data, real_off)[0]
            inv_refs[name] = ptr
    result["inv_refs"] = {k: hex(v) if v else "null" for k, v in inv_refs.items()}

    # 2. Read boosters list to see booster delta
    boosters_ptr = inv_refs.get("boosters", 0)
    if boosters_ptr and 0x1_0000_0000 <= boosters_ptr <= 0x7FFF_FFFF_FFFF:
        list_data = read_memory(pm, boosters_ptr, 32)
        if list_data:
            items_ptr = struct.unpack_from("<Q", list_data, 16)[0]
            size = struct.unpack_from("<i", list_data, 24)[0]
            result["boosters"] = {"items_ptr": hex(items_ptr), "size": size}

            # Read booster elements
            if items_ptr and 0x1_0000_0000 <= items_ptr <= 0x7FFF_FFFF_FFFF:
                arr_data = read_memory(pm, items_ptr, 32 + size * 8)
                if arr_data:
                    booster_entries = []
                    for i in range(size):
                        elem_ptr = struct.unpack_from("<Q", arr_data, 32 + i * 8)[0]
                        if elem_ptr and 0x1_0000_0000 <= elem_ptr <= 0x7FFF_FFFF_FFFF:
                            elem = read_memory(pm, elem_ptr, 32)
                            if elem:
                                coll_id = struct.unpack_from("<i", elem, 16)[0]
                                count = struct.unpack_from("<i", elem, 20)[0]
                                booster_entries.append({
                                    "addr": hex(elem_ptr),
                                    "collation_id": coll_id,
                                    "count": count,
                                })
                    result["booster_entries"] = booster_entries

    # 3. Global search for CollationId 100032 with Count < 0 (new delta boosters)
    target = 100032
    target_bytes = struct.pack("<i", target)
    chunk_size = 4 * 1024 * 1024
    delta_boosters = []

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
                idx = data.find(target_bytes, pos)
                if idx == -1:
                    break
                addr = region.base + offset + idx
                # Check for negative count at +4
                if idx + 4 < len(data):
                    count = struct.unpack_from("<i", data, idx + 4)[0]
                    if count < 0:
                        # Read klass at -16
                        ctx = read_memory(pm, addr - 16, 32)
                        if ctx:
                            klass = struct.unpack_from("<Q", ctx, 0)[0]
                            monitor = struct.unpack_from("<Q", ctx, 8)[0]
                            if (0x1_0000_0000 <= klass <= 0x7FFF_FFFF_FFFF) and monitor == 0:
                                delta_boosters.append({
                                    "obj_addr": hex(addr - 16),
                                    "klass": hex(klass),
                                    "collation_id": target,
                                    "count": count,
                                })
                pos = idx + 1
            offset += read_size

    result["delta_boosters"] = delta_boosters

    # 4. Search for ALL pointers to each delta booster and read parent context
    for db in delta_boosters:
        obj_addr = int(db["obj_addr"], 16)
        target_bytes_ptr = struct.pack("<Q", obj_addr)
        parents = []

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
                    idx = data.find(target_bytes_ptr, pos)
                    if idx == -1:
                        break
                    ptr_loc = region.base + offset + idx
                    # Read context around the pointer
                    ctx = read_memory(pm, ptr_loc - 64, 192)
                    if ctx:
                        parent_info = {
                            "ptr_loc": hex(ptr_loc),
                            "context": [],
                        }
                        # Find object header before the pointer
                        for back in range(8, 65, 8):
                            check_off = 64 - back
                            ck = struct.unpack_from("<Q", ctx, check_off)[0]
                            cm = struct.unpack_from("<Q", ctx, check_off + 8)[0]
                            if (0x1_0000_0000 <= ck <= 0x7FFF_FFFF_FFFF) and cm == 0:
                                parent_info["parent_obj"] = hex(ptr_loc - back)
                                parent_info["parent_klass"] = hex(ck)
                                parent_info["ptr_offset_in_parent"] = back

                                # Read the parent object in full
                                parent_full = read_memory(pm, ptr_loc - back, 128)
                                if parent_full:
                                    fields = []
                                    for fi in range(0, 128, 8):
                                        fv = struct.unpack_from("<Q", parent_full, fi)[0]
                                        fi32a = struct.unpack_from("<i", parent_full, fi)[0]
                                        fi32b = struct.unpack_from("<i", parent_full, fi + 4)[0]
                                        is_ptr = 0x1_0000_0000 <= fv <= 0x7FFF_FFFF_FFFF
                                        s = read_string(pm, fv) if is_ptr else None
                                        fields.append({
                                            "offset": fi,
                                            "type": "ptr" if is_ptr else "null" if fv == 0 else "i32",
                                            "value": hex(fv) if is_ptr else (0 if fv == 0 else [fi32a, fi32b]),
                                            **({"string": s} if s else {}),
                                        })
                                    parent_info["fields"] = fields
                                break

                        parents.append(parent_info)
                    pos = idx + 1
                offset += read_size

        db["parents"] = parents

    # 5. If there are delta boosters, read the allocation region around them
    if delta_boosters:
        first_obj = int(delta_boosters[0]["obj_addr"], 16)
        region_start = first_obj - 512
        region_data = read_memory(pm, region_start, 2048)
        if region_data:
            # Find all objects in this region
            objects_in_region = []
            i = 0
            while i < len(region_data) - 16:
                klass = struct.unpack_from("<Q", region_data, i)[0]
                monitor = struct.unpack_from("<Q", region_data, i + 8)[0]
                if (0x1_0000_0000 <= klass <= 0x7FFF_FFFF_FFFF) and monitor == 0:
                    addr = region_start + i
                    vals = [struct.unpack_from("<i", region_data, i + 16 + j)[0]
                            for j in range(0, min(24, len(region_data) - i - 16), 4)]
                    objects_in_region.append({
                        "addr": hex(addr),
                        "klass": hex(klass),
                        "vals": vals,
                    })
                    i += 16
                else:
                    i += 8
            result["objects_near_delta"] = objects_in_region

    return result


def main() -> int:
    parser = argparse.ArgumentParser(description="Burst capture on inventory change")
    parser.add_argument("--gold", type=int, default=None)
    parser.add_argument("--timeout", type=int, default=120, help="Max wait time in seconds")
    args = parser.parse_args()

    sh = extract_starthook()
    known = {
        "gems": sh.gems,
        "wc_common": sh.wc_common,
        "wc_uncommon": sh.wc_uncommon,
        "wc_rare": sh.wc_rare,
        "wc_track_position": sh.wc_track_position,
    }
    if args.gold is not None:
        known["gold"] = args.gold
    known = {k: v for k, v in known.items() if v != 0}

    print("Opening MTGA.exe...")
    pm = open_mtga()
    print(f"  PID: {pm.process_id}")

    regions = enumerate_regions(pm)
    print(f"  {len(regions)} regions")

    print(f"Finding inventory ({len(known)} anchors)...")
    location = find_inventory(pm, regions, known)
    if location is None:
        print("ERROR: Inventory not found", file=sys.stderr)
        pm.close_process()
        return 1

    inv_addr = location.address
    before = read_scalars(pm, inv_addr)
    if before is None:
        print("ERROR: Failed to read inventory", file=sys.stderr)
        pm.close_process()
        return 1

    print(f"  Inventory at {inv_addr:#x}")
    print(f"  gold={before['gold']}, gems={before['gems']}, vault={before['vault_raw']}")
    print()
    print("=" * 60)
    print("WATCHING — open a pack or perform an action NOW")
    print(f"Will capture for {args.timeout}s max. Ctrl+C to stop.")
    print("=" * 60)

    poll_count = 0
    captures = []
    start_time = time.time()

    try:
        while time.time() - start_time < args.timeout:
            time.sleep(0.05)
            poll_count += 1

            current = read_scalars(pm, inv_addr)
            if current is None:
                continue

            if current != before:
                change_time = time.time() - start_time
                print(f"\n*** CHANGE #{len(captures)+1} at {change_time:.1f}s (poll #{poll_count}) ***")
                for k in before:
                    if before[k] != current[k]:
                        print(f"  {k}: {before[k]} -> {current[k]}")

                # Re-enumerate regions (new objects may be in new pages)
                regions = enumerate_regions(pm)

                print("  Capturing... ", end="", flush=True)
                capture = burst_capture(pm, regions, inv_addr, before, current, change_time)
                captures.append(capture)
                print("done.")

                # Report key findings
                n_delta = len(capture.get("delta_boosters", []))
                print(f"  Delta boosters found: {n_delta}")
                for db in capture.get("delta_boosters", []):
                    n_parents = len(db.get("parents", []))
                    print(f"    {db['obj_addr']}: count={db['count']}, {n_parents} parent pointers")
                    for p in db.get("parents", []):
                        if "parent_obj" in p:
                            print(f"      Parent at {p['parent_obj']} (klass={p['parent_klass']}, "
                                  f"ptr at obj+{p['ptr_offset_in_parent']})")

                before = current
                print("  Continuing to watch...")

    except KeyboardInterrupt:
        print("\nStopped by user.")

    if captures:
        outfile = Path("python/research/burst_output.json")
        with open(outfile, "w") as f:
            json.dump(captures, f, indent=2)
        print(f"\nSaved {len(captures)} capture(s) to {outfile}")
    else:
        print("\nNo changes detected.")

    pm.close_process()
    return 0


if __name__ == "__main__":
    sys.exit(main())
