"""Memory research CLI — tools for discovering MTGA memory layouts.

Usage:
    python python/research/main.py probe         Find inventory and show scalars
    python python/research/main.py dump           Dump memory around inventory struct
    python python/research/main.py compare        Compare memory vs StartHook
    python python/research/main.py follow         Follow a pointer from inventory base
    python python/research/main.py search         Search for integer value near an address
    python python/research/main.py starthook      Show all known StartHook values
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
from .starthook import StartHookValues, extract_starthook, format_starthook
from .analyzer import (
    annotate_extended_dump,
    compare_with_starthook,
    detect_pointers,
    hex_dump,
)


def _get_known_values(sh: StartHookValues, overrides: dict[str, int] | None = None) -> dict[str, int]:
    """Extract non-zero scalar values for inventory search."""
    candidates = {
        "gold": sh.gold,
        "gems": sh.gems,
        "wc_common": sh.wc_common,
        "wc_uncommon": sh.wc_uncommon,
        "wc_rare": sh.wc_rare,
        "wc_mythic": sh.wc_mythic,
        "wc_track_position": sh.wc_track_position,
    }
    if overrides:
        candidates.update(overrides)
    return {k: v for k, v in candidates.items() if v != 0}


def _find_inventory_or_exit(sh: StartHookValues, overrides: dict[str, int] | None = None) -> tuple:
    """Open process, find inventory, return (pm, regions, location). Exits on failure."""
    known = _get_known_values(sh, overrides)
    if len(known) < 3:
        print(f"ERROR: Only {len(known)} non-zero known values. Need at least 3.", file=sys.stderr)
        print("Values may have changed since StartHook. Try restarting MTGA.", file=sys.stderr)
        sys.exit(1)

    print(f"Opening MTGA.exe...")
    pm = open_mtga()
    print(f"  PID: {pm.process_id}")

    print(f"Enumerating memory regions...")
    regions = enumerate_regions(pm)
    print(f"  {len(regions)} readable regions")

    print(f"Searching for inventory ({len(known)} anchor values)...")
    start = time.perf_counter()
    location = find_inventory(pm, regions, known)
    elapsed = time.perf_counter() - start

    if location is None:
        print(f"ERROR: Inventory not found ({elapsed:.1f}s). Values may have changed.", file=sys.stderr)
        pm.close_process()
        sys.exit(1)

    print(f"  Found at {location.address:#x} (score={location.score}, "
          f"candidates={location.candidates_found}, {elapsed:.1f}s)")

    return pm, regions, location


def cmd_starthook(args: argparse.Namespace) -> int:
    """Show all known StartHook values."""
    sh = extract_starthook()
    print(format_starthook(sh))
    return 0


def cmd_probe(args: argparse.Namespace) -> int:
    """Find inventory struct and display scalars."""
    sh = extract_starthook()
    overrides = {}
    if args.gold is not None:
        overrides["gold"] = args.gold
    pm, regions, location = _find_inventory_or_exit(sh, overrides or None)

    print(f"\nInventory Scalars (from memory):")
    for k, v in location.scalars.items():
        print(f"  {k:20s} = {v}")

    print()
    print(compare_with_starthook(location.scalars, sh))

    pm.close_process()
    return 0


def cmd_dump(args: argparse.Namespace) -> int:
    """Dump memory around the inventory struct."""
    sh = extract_starthook()
    pm, regions, location = _find_inventory_or_exit(sh)

    size = args.size
    offset = args.offset

    if args.address:
        dump_addr = int(args.address, 16) if args.address.startswith("0x") else int(args.address)
    else:
        dump_addr = location.address + offset

    print(f"\nDumping {size} bytes from {dump_addr:#x} (inventory base + {dump_addr - location.address})")

    data = read_memory(pm, dump_addr, size)
    if data is None:
        print("ERROR: Failed to read memory at that address.", file=sys.stderr)
        pm.close_process()
        return 1

    # Use annotated dump for memory after the scalar region
    if dump_addr >= location.address + 40:
        print(annotate_extended_dump(data, dump_addr, sh, pm))
    else:
        known = _get_known_values(sh)
        print(hex_dump(data, dump_addr, known))

    pm.close_process()
    return 0


def cmd_compare(args: argparse.Namespace) -> int:
    """Compare memory scalars against StartHook values."""
    sh = extract_starthook()
    pm, regions, location = _find_inventory_or_exit(sh)

    print()
    print(compare_with_starthook(location.scalars, sh))

    pm.close_process()
    return 0


def cmd_follow(args: argparse.Namespace) -> int:
    """Follow a pointer at a given offset from inventory base."""
    sh = extract_starthook()
    pm, regions, location = _find_inventory_or_exit(sh)

    ptr_addr = location.address + args.offset
    print(f"\nReading pointer at inventory+{args.offset} ({ptr_addr:#x})...")

    ptr_value = read_i64(pm, ptr_addr)
    if ptr_value is None:
        print("ERROR: Failed to read pointer.", file=sys.stderr)
        pm.close_process()
        return 1

    print(f"  Pointer value: {ptr_value:#x}")

    if ptr_value == 0:
        print("  NULL pointer — field is empty.")
        pm.close_process()
        return 0

    if not (0x1_0000_0000 <= ptr_value <= 0x7FFF_FFFF_FFFF):
        print(f"  Not a valid heap pointer (out of range).")
        pm.close_process()
        return 0

    read_size = args.size
    print(f"\nReading {read_size} bytes at target {ptr_value:#x}:")

    target_data = read_memory(pm, ptr_value, read_size)
    if target_data is None:
        print("  ERROR: Failed to read target memory.", file=sys.stderr)
        pm.close_process()
        return 1

    print(annotate_extended_dump(target_data, ptr_value, sh, pm))

    pm.close_process()
    return 0


def cmd_search(args: argparse.Namespace) -> int:
    """Search for a specific integer value near an address."""
    sh = extract_starthook()
    pm, regions, location = _find_inventory_or_exit(sh)

    center = location.address if args.near is None else args.near
    radius = args.radius
    value = args.value
    value_bytes = value.to_bytes(4, "little", signed=True)

    start_addr = max(0, center - radius)
    search_size = radius * 2

    print(f"\nSearching for i32={value} ({value_bytes.hex()}) "
          f"within {radius} bytes of {center:#x}...")

    data = read_memory(pm, start_addr, search_size)
    if data is None:
        print("ERROR: Failed to read search region.", file=sys.stderr)
        pm.close_process()
        return 1

    hits: list[int] = []
    pos = 0
    while True:
        idx = data.find(value_bytes, pos)
        if idx == -1:
            break
        hit_addr = start_addr + idx
        hits.append(hit_addr)
        pos = idx + 1

    print(f"  Found {len(hits)} occurrences:")
    for addr in hits[:50]:
        rel = addr - location.address
        print(f"    {addr:#x}  (inventory base + {rel:+d})")

    pm.close_process()
    return 0


def cmd_global_search(args: argparse.Namespace) -> int:
    """Search ALL readable memory regions for an integer value."""
    print(f"Opening MTGA.exe...")
    pm = open_mtga()
    print(f"  PID: {pm.process_id}")

    print(f"Enumerating memory regions...")
    regions = enumerate_regions(pm)
    print(f"  {len(regions)} readable regions")

    value = args.value
    value_bytes = value.to_bytes(4, "little", signed=True)
    chunk_size = 4 * 1024 * 1024  # 4MB chunks

    print(f"\nGlobal search for i32={value} ({value_bytes.hex()}) across all regions...")

    hits: list[int] = []
    regions_scanned = 0
    bytes_scanned = 0

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
                hit_addr = region.base + offset + idx
                hits.append(hit_addr)
                pos = idx + 1

            bytes_scanned += read_size
            offset += read_size
        regions_scanned += 1

    print(f"  Scanned {regions_scanned} regions ({bytes_scanned / 1024 / 1024:.1f} MB)")
    print(f"  Found {len(hits)} occurrences:")

    for addr in hits[:100]:
        # Read 32 bytes of context around each hit
        context = read_memory(pm, addr - 8, 48)
        context_str = ""
        if context:
            # Show the i32 values around the hit
            vals = []
            for i in range(0, min(48, len(context)), 4):
                v = struct.unpack_from("<i", context, i)[0]
                vals.append(v)
            context_str = f"  context: {vals}"
        print(f"    {addr:#x}{context_str}")

    pm.close_process()
    return 0


def cmd_deep_search(args: argparse.Namespace) -> int:
    """Follow all pointers from inventory and search for a value in their targets."""
    sh = extract_starthook()
    pm, regions, location = _find_inventory_or_exit(sh)

    target_value = args.value
    target_bytes = struct.pack("<i", target_value)
    max_depth = args.depth
    search_size = args.size

    print(f"\nDeep search for i32={target_value} starting from inventory base")
    print(f"  Max depth: {max_depth}, search size per target: {search_size}")

    # Collect all non-null pointers from inventory struct (scan a wide range)
    # Inventory object extends to 768+ bytes — must scan the full object
    inv_data = read_memory(pm, location.address, 1024)
    if inv_data is None:
        print("ERROR: Failed to read inventory.", file=sys.stderr)
        pm.close_process()
        return 1

    visited: set[int] = set()
    found: list[tuple[str, int, int]] = []  # (path, address_of_value, offset_in_target)

    def search_at(addr: int, depth: int, path: str) -> None:
        if addr in visited or depth > max_depth:
            return
        visited.add(addr)

        data = read_memory(pm, addr, search_size)
        if data is None:
            return

        # Search for target value in this memory
        pos = 0
        while True:
            idx = data.find(target_bytes, pos)
            if idx == -1:
                break
            found.append((path, addr + idx, idx))
            pos = idx + 1

        # If depth allows, follow pointers in this memory
        if depth < max_depth:
            for i in range(0, len(data) - 7, 8):
                ptr_val = struct.unpack_from("<Q", data, i)[0]
                if 0x1_0000_0000 <= ptr_val <= 0x7FFF_FFFF_FFFF:
                    child_path = f"{path} -> [{i}]"
                    search_at(ptr_val, depth + 1, child_path)

    # Start from each pointer in the inventory struct
    for offset in range(40, 1024, 8):
        if offset + 8 > len(inv_data):
            break
        ptr_val = struct.unpack_from("<Q", inv_data, offset)[0]
        if ptr_val == 0:
            continue
        if not (0x1_0000_0000 <= ptr_val <= 0x7FFF_FFFF_FFFF):
            continue
        path = f"inv+{offset}"
        search_at(ptr_val, 1, path)

    print(f"\n  Visited {len(visited)} unique addresses")
    print(f"  Found {len(found)} occurrences of {target_value}:\n")

    for path, addr, offset in found[:30]:
        print(f"  {path}  at target+{offset} ({addr:#x})")

    pm.close_process()
    return 0


def cmd_ptrscan(args: argparse.Namespace) -> int:
    """Find all pointers in memory that point to a target address range."""
    print(f"Opening MTGA.exe...")
    pm = open_mtga()
    print(f"  PID: {pm.process_id}")

    print(f"Enumerating memory regions...")
    regions = enumerate_regions(pm)
    print(f"  {len(regions)} readable regions")

    target = int(args.target, 0)
    margin = args.margin
    chunk_size = 4 * 1024 * 1024

    print(f"\nSearching for pointers to {target:#x} (±{margin} bytes)...")
    print(f"  Target range: {target - margin:#x} — {target + margin:#x}")

    hits: list[tuple[int, int]] = []  # (pointer_location, pointer_value)
    regions_scanned = 0

    for region in regions:
        offset = 0
        while offset < region.size:
            read_size = min(chunk_size, region.size - offset)
            # Need at least 8 bytes for a pointer
            if read_size < 8:
                offset += read_size
                continue
            try:
                data = pm.read_bytes(region.base + offset, read_size)
            except Exception:
                offset += read_size
                continue

            # Scan for 8-byte pointer values in the target range
            for i in range(0, len(data) - 7, 8):  # 8-byte aligned
                ptr_val = struct.unpack_from("<Q", data, i)[0]
                if target - margin <= ptr_val <= target + margin:
                    loc = region.base + offset + i
                    hits.append((loc, ptr_val))

            offset += read_size
        regions_scanned += 1

    print(f"  Scanned {regions_scanned} regions")
    print(f"  Found {len(hits)} pointer(s):\n")

    # Find inventory for relative offset display
    sh = extract_starthook()
    known = _get_known_values(sh)
    inv_location = None
    if len(known) >= 3:
        inv_location = find_inventory(pm, regions, known)

    for loc, val in hits[:50]:
        inv_rel = ""
        if inv_location:
            rel = loc - inv_location.address
            if -1024 <= rel <= 1024:
                inv_rel = f"  [inv+{rel}]"
        # Read context around the pointer location
        ctx = read_memory(pm, loc - 16, 48)
        ctx_str = ""
        if ctx:
            vals = []
            for j in range(0, min(48, len(ctx)), 4):
                v = struct.unpack_from("<i", ctx, j)[0]
                vals.append(v)
            ctx_str = f"\n    context: {vals}"
        print(f"  {loc:#x} -> {val:#x}{inv_rel}{ctx_str}")

    pm.close_process()
    return 0


def cmd_listfind(args: argparse.Namespace) -> int:
    """Find List<T> objects with a specific _size reachable from inventory pointers.

    Scans all 8-byte-aligned pointer slots in the inventory struct (up to 1024 bytes),
    dereferences each, and checks if the target looks like a .NET List<T> with _size == target.
    Then follows the _items array pointer and dumps the elements.
    """
    sh = extract_starthook()
    pm, regions, location = _find_inventory_or_exit(sh)

    target_size = args.list_size
    inv_scan = args.inv_range

    print(f"\nSearching inventory pointers for List<T> with _size={target_size}")
    print(f"  Inventory base: {location.address:#x}")
    print(f"  Scan range: inv+40 to inv+{inv_scan}")

    inv_data = read_memory(pm, location.address, inv_scan)
    if inv_data is None:
        print("ERROR: Failed to read inventory.", file=sys.stderr)
        pm.close_process()
        return 1

    def check_list_at(ptr_val: int, label: str) -> bool:
        """Check if ptr_val points to a List<T> with _size == target_size."""
        target_data = read_memory(pm, ptr_val, 40)
        if target_data is None:
            return False

        hit = False
        # Try two IL2CPP layouts:
        # Layout A: +0 klass, +8 _items, +16 _size, +20 _version
        # Layout B: +0 klass, +8 monitor, +16 _items, +24 _size, +28 _version
        for layout_name, items_off, size_off, ver_off in [
            ("A", 8, 16, 20),
            ("B", 16, 24, 28),
        ]:
            if size_off + 4 > len(target_data):
                continue
            items_ptr = struct.unpack_from("<Q", target_data, items_off)[0]
            size_val = struct.unpack_from("<i", target_data, size_off)[0]
            ver_val = struct.unpack_from("<i", target_data, ver_off)[0]

            if size_val != target_size:
                continue
            if items_ptr == 0 or not (0x1_0000_0000 <= items_ptr <= 0x7FFF_FFFF_FFFF):
                continue
            if ver_val < 0 or ver_val > 1_000_000:
                continue

            klass = struct.unpack_from("<Q", target_data, 0)[0]
            print(f"\n  HIT {label} -> {ptr_val:#x} (layout {layout_name})")
            print(f"    klass:   {klass:#x}")
            print(f"    _items:  {items_ptr:#x}")
            print(f"    _size:   {size_val}")
            print(f"    _version: {ver_val}")

            # Follow _items and dump elements
            array_data = read_memory(pm, items_ptr, 256)
            if array_data is not None:
                arr_klass = struct.unpack_from("<Q", array_data, 0)[0]
                arr_len_raw = struct.unpack_from("<Q", array_data, 8)[0]
                print(f"    Array klass:  {arr_klass:#x}")
                print(f"    Array length: {arr_len_raw}")
                print(f"    Array raw bytes +16 to +128:")
                for i in range(16, min(128, len(array_data)), 8):
                    val_u64 = struct.unpack_from("<Q", array_data, i)[0]
                    val_i32_a = struct.unpack_from("<i", array_data, i)[0]
                    val_i32_b = struct.unpack_from("<i", array_data, i + 4)[0]
                    if 0x1_0000_0000 <= val_u64 <= 0x7FFF_FFFF_FFFF:
                        print(f"      +{i:3d}: PTR {val_u64:#x}")
                        elem = read_memory(pm, val_u64, 64)
                        if elem:
                            vals = [struct.unpack_from("<i", elem, j)[0] for j in range(0, 64, 4)]
                            print(f"             -> {vals}")
                    else:
                        print(f"      +{i:3d}: {val_i32_a}, {val_i32_b}")

            hits.append((label, ptr_val, size_val))
            hit = True
        return hit

    hits: list[tuple[str, int, int]] = []

    # Depth 1: direct inventory pointers
    depth1_ptrs = []
    for offset in range(40, inv_scan, 8):
        if offset + 8 > len(inv_data):
            break
        ptr_val = struct.unpack_from("<Q", inv_data, offset)[0]
        if ptr_val == 0 or not (0x1_0000_0000 <= ptr_val <= 0x7FFF_FFFF_FFFF):
            continue
        depth1_ptrs.append((offset, ptr_val))
        check_list_at(ptr_val, f"inv+{offset}")

    # Depth 2: follow each depth-1 pointer and check its sub-pointers
    if not hits:
        print(f"\n  No depth-1 hits. Checking depth 2 ({len(depth1_ptrs)} pointers)...")
        for inv_off, d1_ptr in depth1_ptrs:
            d1_data = read_memory(pm, d1_ptr, 512)
            if d1_data is None:
                continue
            for sub_off in range(0, 512, 8):
                if sub_off + 8 > len(d1_data):
                    break
                sub_ptr = struct.unpack_from("<Q", d1_data, sub_off)[0]
                if sub_ptr == 0 or not (0x1_0000_0000 <= sub_ptr <= 0x7FFF_FFFF_FFFF):
                    continue
                check_list_at(sub_ptr, f"inv+{inv_off}->[{sub_off}]")

    print(f"\n  Total List<T> hits with _size={target_size}: {len(hits)}")
    pm.close_process()
    return 0


def cmd_snapshot(args: argparse.Namespace) -> int:
    """Snapshot inventory struct + all pointer targets for before/after diffing.

    Reads the full inventory struct (1024 bytes), follows every non-null pointer,
    and reads 128 bytes at each target. Saves to a file for later comparison.
    """
    import json as _json
    sh = extract_starthook()
    overrides = {}
    if args.gold is not None:
        overrides["gold"] = args.gold
    pm, regions, location = _find_inventory_or_exit(sh, overrides or None)

    inv_size = 1024
    target_read = 256  # bytes to read at each pointer target
    depth2_read = 128  # bytes to read at depth-2 targets

    print(f"\nSnapshot inventory at {location.address:#x}")

    inv_data = read_memory(pm, location.address, inv_size)
    if inv_data is None:
        print("ERROR: Failed to read inventory.", file=sys.stderr)
        pm.close_process()
        return 1

    snapshot: dict = {
        "inv_address": location.address,
        "scalars": location.scalars,
        "pointers": {},
    }

    # Read all pointer slots from the inventory
    for offset in range(0, inv_size, 8):
        if offset + 8 > len(inv_data):
            break
        val = struct.unpack_from("<Q", inv_data, offset)[0]
        if val == 0:
            continue
        if not (0x1_0000_0000 <= val <= 0x7FFF_FFFF_FFFF):
            # Store non-pointer i32 values for the scalar region
            if offset < 40:
                continue
            continue

        # Read target memory
        target_data = read_memory(pm, val, target_read)
        if target_data is None:
            snapshot["pointers"][str(offset)] = {
                "ptr": val,
                "readable": False,
            }
            continue

        # Extract i32 values from target
        i32_vals = []
        for i in range(0, len(target_data), 4):
            i32_vals.append(struct.unpack_from("<i", target_data, i)[0])

        # Follow depth-2 pointers from this target
        depth2 = {}
        for sub_off in range(0, len(target_data), 8):
            sub_val = struct.unpack_from("<Q", target_data, sub_off)[0]
            if sub_val == 0 or not (0x1_0000_0000 <= sub_val <= 0x7FFF_FFFF_FFFF):
                continue
            d2_data = read_memory(pm, sub_val, depth2_read)
            if d2_data is not None:
                d2_vals = []
                for i in range(0, len(d2_data), 4):
                    d2_vals.append(struct.unpack_from("<i", d2_data, i)[0])
                depth2[str(sub_off)] = {"ptr": sub_val, "i32s": d2_vals}

        snapshot["pointers"][str(offset)] = {
            "ptr": val,
            "readable": True,
            "i32s": i32_vals,
            "depth2": depth2,
        }

    outfile = args.output
    with open(outfile, "w") as f:
        _json.dump(snapshot, f, indent=2)

    ptr_count = sum(1 for v in snapshot["pointers"].values() if v.get("readable"))
    d2_count = sum(
        len(v.get("depth2", {}))
        for v in snapshot["pointers"].values()
        if v.get("readable")
    )
    print(f"  Saved {ptr_count} pointer targets ({d2_count} depth-2 targets) to {outfile}")
    pm.close_process()
    return 0


def cmd_diff_snapshots(args: argparse.Namespace) -> int:
    """Compare two snapshots to find what changed."""
    import json as _json

    with open(args.before) as f:
        before = _json.load(f)
    with open(args.after) as f:
        after = _json.load(f)

    print(f"\nDiff: {args.before} vs {args.after}")

    # Compare scalars
    for key in before["scalars"]:
        bval = before["scalars"][key]
        aval = after["scalars"].get(key)
        if bval != aval:
            print(f"  SCALAR {key}: {bval} -> {aval}")

    # Compare pointer targets
    all_offsets = sorted(set(before["pointers"].keys()) | set(after["pointers"].keys()), key=int)

    for offset in all_offsets:
        bp = before["pointers"].get(offset, {})
        ap = after["pointers"].get(offset, {})

        if not bp:
            print(f"  inv+{offset}: NEW pointer {ap.get('ptr', '?'):#x}")
            continue
        if not ap:
            print(f"  inv+{offset}: REMOVED pointer {bp.get('ptr', '?'):#x}")
            continue

        # Compare pointer values
        if bp.get("ptr") != ap.get("ptr"):
            print(f"  inv+{offset}: ptr changed {bp['ptr']:#x} -> {ap['ptr']:#x}")

        # Compare i32 values at depth 1
        b_vals = bp.get("i32s", [])
        a_vals = ap.get("i32s", [])
        if b_vals != a_vals:
            changes = []
            for i, (bv, av) in enumerate(zip(b_vals, a_vals)):
                if bv != av:
                    changes.append(f"+{i*4}: {bv}->{av}")
            if len(b_vals) != len(a_vals):
                changes.append(f"len: {len(b_vals)}->{len(a_vals)}")
            if changes:
                print(f"  inv+{offset} target CHANGED: {', '.join(changes[:10])}")

        # Compare depth-2
        b_d2 = bp.get("depth2", {})
        a_d2 = ap.get("depth2", {})
        d2_offsets = sorted(set(b_d2.keys()) | set(a_d2.keys()), key=int)
        for d2off in d2_offsets:
            bd2 = b_d2.get(d2off, {})
            ad2 = a_d2.get(d2off, {})
            if bd2.get("i32s") != ad2.get("i32s"):
                if not bd2:
                    print(f"  inv+{offset}->[{d2off}]: NEW depth-2 target")
                elif not ad2:
                    print(f"  inv+{offset}->[{d2off}]: REMOVED depth-2 target")
                else:
                    d2_changes = []
                    bv2 = bd2.get("i32s", [])
                    av2 = ad2.get("i32s", [])
                    for i, (bv, av) in enumerate(zip(bv2, av2)):
                        if bv != av:
                            d2_changes.append(f"+{i*4}: {bv}->{av}")
                    if d2_changes:
                        print(f"  inv+{offset}->[{d2off}] depth-2 CHANGED: {', '.join(d2_changes[:8])}")

    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="memory-research",
        description="MTGA memory research tools — discover IL2CPP object layouts",
    )
    sub = parser.add_subparsers(dest="command", required=True)

    sub.add_parser("starthook", help="Show all known StartHook values")
    p_probe = sub.add_parser("probe", help="Find inventory struct and display scalars")
    p_probe.add_argument("--gold", type=int, default=None, help="Override gold value (if changed since StartHook)")
    sub.add_parser("compare", help="Compare memory scalars vs StartHook values")

    p_dump = sub.add_parser("dump", help="Dump memory around inventory struct")
    p_dump.add_argument("--address", type=str, help="Absolute address to dump (hex or decimal)")
    p_dump.add_argument("--offset", type=int, default=0, help="Offset from inventory base (default: 0)")
    p_dump.add_argument("--size", type=int, default=512, help="Bytes to dump (default: 512)")

    p_follow = sub.add_parser("follow", help="Follow a pointer at an offset from inventory base")
    p_follow.add_argument("--offset", type=int, required=True, help="Byte offset of pointer from inventory base")
    p_follow.add_argument("--size", type=int, default=256, help="Bytes to read at target (default: 256)")

    p_search = sub.add_parser("search", help="Search for an integer value near an address")
    p_search.add_argument("--value", type=int, required=True, help="i32 value to search for")
    p_search.add_argument("--near", type=lambda x: int(x, 0), default=None,
                          help="Center address (default: inventory base)")
    p_search.add_argument("--radius", type=int, default=65536, help="Search radius in bytes (default: 65536)")

    p_gsearch = sub.add_parser("gsearch", help="Search ALL memory for an integer value")
    p_gsearch.add_argument("--value", type=int, required=True, help="i32 value to search for")

    p_deep = sub.add_parser("deepsearch", help="Follow pointers from inventory, search for value")
    p_deep.add_argument("--value", type=int, required=True, help="i32 value to find")
    p_deep.add_argument("--depth", type=int, default=2, help="Max pointer depth (default: 2)")
    p_deep.add_argument("--size", type=int, default=2048, help="Bytes to search per target (default: 2048)")

    p_ptrscan = sub.add_parser("ptrscan", help="Find pointers to a target address")
    p_ptrscan.add_argument("--target", type=str, required=True, help="Target address (hex)")
    p_ptrscan.add_argument("--margin", type=int, default=256, help="Match range ± bytes (default: 256)")

    p_listfind = sub.add_parser("listfind", help="Find List<T> objects with specific _size from inventory")
    p_listfind.add_argument("--list-size", type=int, required=True, help="Expected List._size value")
    p_listfind.add_argument("--inv-range", type=int, default=1024, help="Inventory scan range (default: 1024)")

    p_snap = sub.add_parser("snapshot", help="Snapshot inventory pointers for before/after diffing")
    p_snap.add_argument("--output", type=str, required=True, help="Output JSON file path")
    p_snap.add_argument("--gold", type=int, default=None, help="Override gold value (if changed since StartHook)")

    p_diff = sub.add_parser("diff", help="Compare two snapshots to find changes")
    p_diff.add_argument("--before", type=str, required=True, help="Before snapshot JSON")
    p_diff.add_argument("--after", type=str, required=True, help="After snapshot JSON")

    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()

    dispatch = {
        "starthook": cmd_starthook,
        "probe": cmd_probe,
        "compare": cmd_compare,
        "dump": cmd_dump,
        "follow": cmd_follow,
        "search": cmd_search,
        "gsearch": cmd_global_search,
        "deepsearch": cmd_deep_search,
        "ptrscan": cmd_ptrscan,
        "listfind": cmd_listfind,
        "snapshot": cmd_snapshot,
        "diff": cmd_diff_snapshots,
    }

    handler = dispatch.get(args.command)
    if handler is None:
        parser.print_help()
        return 1

    return handler(args)


if __name__ == "__main__":
    sys.exit(main())
