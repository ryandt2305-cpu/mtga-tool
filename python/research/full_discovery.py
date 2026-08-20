"""Full discovery — locate ALL known memory targets in one pass.

This is the Python reference for the Rust MemoryMonitor's discovery.rs.
Finds all 7 verified targets and outputs a JSON state report.

Targets:
  1. ClientPlayerInventory scalars (anchor search)
  2. CustomTokens Dict<string,int> (inventory nav: inv-8)
  3. Boosters List<ClientBoosterInfo> (inventory nav: inv-64)
  4. InventoryClient parent → Report Queue (pointer search for inv_obj_base)
  5. CosmeticsClient (6-list pattern match, global scan)
  6. Rank State (season_ordinal anchor, global scan)
  7. Mastery LevelTrack (progress anchor, global scan)

Run from mtga-tool/: python -m python.research.full_discovery [--progress VALUE]
"""
from __future__ import annotations

import argparse
import json
import struct
import sys
import time

from .probe import (
    MemRegion,
    enumerate_regions,
    find_inventory,
    open_mtga,
    read_i32,
    read_i64,
    read_memory,
)
from .starthook import extract_starthook, StartHookValues
from .verify import read_il2cpp_string


# ── Target 1: Inventory Scalars ──────────────────────────────────────

def discover_inventory(pm, regions, sh: StartHookValues) -> dict | None:
    """Find ClientPlayerInventory scalars via anchor search."""
    known = {
        "gold": sh.gold,
        "gems": sh.gems,
        "wc_common": sh.wc_common,
        "wc_uncommon": sh.wc_uncommon,
        "wc_rare": sh.wc_rare,
        "wc_mythic": sh.wc_mythic,
        "wc_track_position": sh.wc_track_position,
    }
    known = {k: v for k, v in known.items() if v != 0}

    if len(known) < 3:
        return None

    location = find_inventory(pm, regions, known)
    if location is None:
        return None

    return {
        "inv_addr": location.address,
        "inv_obj_base": location.address - 80,
        "score": location.score,
        "candidates": location.candidates_found,
        "scalars": location.scalars,
    }


# ── Target 2: CustomTokens ──────────────────────────────────────────

def discover_custom_tokens(pm, inv_addr: int, sh: StartHookValues) -> dict | None:
    """Navigate from inventory to CustomTokens Dict<string,int> at inv-8."""
    dict_ptr = read_i64(pm, inv_addr - 8)
    if dict_ptr is None or dict_ptr == 0:
        return None
    if not (0x1_0000_0000 <= dict_ptr <= 0x7FFF_FFFF_FFFF):
        return None

    # Read Dict object: klass(8) + monitor(8) + _buckets(8) + _entries(8) +
    #                   _comparer(8) + _keys(8) + _values(8) + _syncRoot(8) +
    #                   _count(4) + _freeList(4) + _freeCount(4) + _version(4)
    dict_data = read_memory(pm, dict_ptr, 80)
    if dict_data is None:
        return None

    klass = struct.unpack_from("<Q", dict_data, 0)[0]
    entries_ptr = struct.unpack_from("<Q", dict_data, 24)[0]
    count = struct.unpack_from("<i", dict_data, 64)[0]

    if count < 0 or count > 100:
        return None

    # Read entries to extract key-value pairs
    tokens = {}
    if entries_ptr and (0x1_0000_0000 <= entries_ptr <= 0x7FFF_FFFF_FFFF):
        # Array header: klass(8) + monitor(8) + bounds(8) + max_length(8) = 32
        arr_header = read_memory(pm, entries_ptr, 32)
        if arr_header:
            max_length = struct.unpack_from("<Q", arr_header, 24)[0]
            if max_length < 1000:
                # Each entry: hashCode(4) + next(4) + key_ptr(8) + value(4) + pad(4) = 24 bytes
                entries_data = read_memory(pm, entries_ptr + 32, min(max_length, 100) * 24)
                if entries_data:
                    for i in range(min(count, max_length)):
                        off = i * 24
                        if off + 24 > len(entries_data):
                            break
                        hash_code = struct.unpack_from("<i", entries_data, off)[0]
                        key_ptr = struct.unpack_from("<Q", entries_data, off + 8)[0]
                        value = struct.unpack_from("<i", entries_data, off + 16)[0]

                        if hash_code < 0:  # unused entry
                            continue

                        key_name = None
                        if key_ptr and (0x1_0000_0000 <= key_ptr <= 0x7FFF_FFFF_FFFF):
                            key_name = read_il2cpp_string(pm, key_ptr)

                        if key_name:
                            tokens[key_name] = value

    # Cross-validate with StartHook
    expected = sh.custom_tokens
    matches = sum(1 for k, v in expected.items() if tokens.get(k) == v) if expected else 0

    return {
        "address": dict_ptr,
        "klass": klass,
        "count": count,
        "tokens": tokens,
        "expected_matches": f"{matches}/{len(expected)}" if expected else "no_ref",
        "valid": matches == len(expected) if expected else count > 0,
    }


# ── Target 3: Boosters ──────────────────────────────────────────────

def discover_boosters(pm, inv_addr: int, sh: StartHookValues) -> dict | None:
    """Navigate from inventory to Boosters List<ClientBoosterInfo> at inv-64."""
    list_ptr = read_i64(pm, inv_addr - 64)
    if list_ptr is None or list_ptr == 0:
        return None
    if not (0x1_0000_0000 <= list_ptr <= 0x7FFF_FFFF_FFFF):
        return None

    # List<T>: klass(8) + monitor(8) + _items(8) + _size(4) + _version(4)
    list_data = read_memory(pm, list_ptr, 32)
    if list_data is None:
        return None

    klass = struct.unpack_from("<Q", list_data, 0)[0]
    items_ptr = struct.unpack_from("<Q", list_data, 16)[0]
    size = struct.unpack_from("<i", list_data, 24)[0]
    version = struct.unpack_from("<i", list_data, 28)[0]

    if size < 0 or size > 100:
        return None

    # Read booster entries
    boosters = []
    if items_ptr and (0x1_0000_0000 <= items_ptr <= 0x7FFF_FFFF_FFFF):
        # Array header = 32 bytes, then size * 8 (pointers to BoosterStack objects)
        arr_data = read_memory(pm, items_ptr, 32 + size * 8)
        if arr_data:
            for i in range(size):
                elem_ptr = struct.unpack_from("<Q", arr_data, 32 + i * 8)[0]
                if elem_ptr == 0 or not (0x1_0000_0000 <= elem_ptr <= 0x7FFF_FFFF_FFFF):
                    continue
                # BoosterStack: klass(8) + monitor(8) + CollationId(4) + Count(4)
                bs_data = read_memory(pm, elem_ptr, 32)
                if bs_data:
                    collation_id = struct.unpack_from("<i", bs_data, 16)[0]
                    count = struct.unpack_from("<i", bs_data, 20)[0]
                    boosters.append({"collation_id": collation_id, "count": count})

    # Cross-validate with StartHook
    expected = sh.boosters  # list of (collation_id, set_code, count)
    matches = 0
    if expected:
        for ecol, eset, ecount in expected:
            for b in boosters:
                if b["collation_id"] == ecol and b["count"] == ecount:
                    matches += 1

    return {
        "list_address": list_ptr,
        "klass": klass,
        "size": size,
        "version": version,
        "boosters": boosters,
        "expected_matches": f"{matches}/{len(expected)}" if expected else "no_ref",
        "valid": (matches == len(expected)) if expected else size >= 0,
    }


# ── Target 4: InventoryClient + Report Queue ────────────────────────

def discover_report_queue(pm, regions, inv_obj_base: int) -> dict | None:
    """Find InventoryClient by searching for pointers to inv_obj_base, then read report queue."""
    target_bytes = struct.pack("<Q", inv_obj_base)
    chunk_size = 4 * 1024 * 1024
    candidates = []

    for region in regions:
        offset = 0
        while offset < region.size:
            read_size = min(chunk_size, region.size - offset)
            if read_size < 8:
                offset += read_size
                continue
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
                ptr_addr = region.base + offset + idx
                obj_base = ptr_addr - 80
                header = read_memory(pm, obj_base, 16)
                if header is not None:
                    klass = struct.unpack_from("<Q", header, 0)[0]
                    monitor = struct.unpack_from("<Q", header, 8)[0]
                    if (0x1_0000_0000 <= klass <= 0x7FFF_FFFF_FFFF) and monitor == 0:
                        list_ptr = read_i64(pm, obj_base + 112)
                        if list_ptr and (0x1_0000_0000 <= list_ptr <= 0x7FFF_FFFF_FFFF):
                            list_data = read_memory(pm, list_ptr, 32)
                            if list_data:
                                list_klass = struct.unpack_from("<Q", list_data, 0)[0]
                                list_size = struct.unpack_from("<i", list_data, 24)[0]
                                list_ver = struct.unpack_from("<i", list_data, 28)[0]
                                if (0x1_0000_0000 <= list_klass <= 0x7FFF_FFFF_FFFF) and 0 <= list_size <= 100:
                                    candidates.append({
                                        "inv_client_addr": obj_base,
                                        "klass": klass,
                                        "queue_list_ptr": list_ptr,
                                        "queue_size": list_size,
                                        "queue_version": list_ver,
                                    })
                pos = idx + 1
            offset += read_size

    if not candidates:
        return None

    # Prefer candidate with non-empty queue that has valid ReportItem elements
    for c in candidates:
        if c["queue_size"] > 0:
            items_ptr_val = read_i64(pm, c["queue_list_ptr"] + 16)
            if items_ptr_val and (0x1_0000_0000 <= items_ptr_val <= 0x7FFF_FFFF_FFFF):
                elem_ptr = read_i64(pm, items_ptr_val + 32)
                if elem_ptr and (0x1_0000_0000 <= elem_ptr <= 0x7FFF_FFFF_FFFF):
                    c["validated"] = True
                    return c

    # Fall back to first candidate
    candidates[0]["validated"] = False
    return candidates[0]


# ── Target 5: CosmeticsClient ───────────────────────────────────────

def discover_cosmetics(pm, regions, sh: StartHookValues) -> dict | None:
    """Find CosmeticsClient via 6-list pattern match (global scan)."""
    expected_sizes = [sh.art_styles_count, sh.avatars_count, sh.pets_count,
                      sh.sleeves_count, sh.emotes_count, sh.titles_count]
    chunk_size = 4 * 1024 * 1024
    candidates = []

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

            for i in range(0, len(data) - 63, 8):
                klass = struct.unpack_from("<Q", data, i)[0]
                monitor = struct.unpack_from("<Q", data, i + 8)[0]

                if monitor != 0:
                    continue
                if not (0x1_0000_0000 <= klass <= 0x7FFF_FFFF_FFFF):
                    continue

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

                # Check first list _size
                first_target = read_memory(pm, ptrs[0], 32)
                if first_target is None:
                    continue
                first_size = struct.unpack_from("<i", first_target, 24)[0]
                if first_size != expected_sizes[0]:
                    continue

                # Check remaining 5
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
                    candidates.append({
                        "address": obj_addr,
                        "klass": klass,
                        "sizes": dict(zip(
                            ["art_styles", "avatars", "pets", "sleeves", "emotes", "titles"],
                            sizes
                        )),
                    })

            offset += read_size

    if len(candidates) == 1:
        candidates[0]["valid"] = True
        return candidates[0]
    elif len(candidates) > 1:
        # Ambiguous — return first but flag
        candidates[0]["valid"] = False
        candidates[0]["ambiguous_count"] = len(candidates)
        return candidates[0]
    return None


# ── Target 6: Rank State ────────────────────────────────────────────

def discover_rank(pm, regions, sh: StartHookValues) -> dict | None:
    """Find Rank State via season_ordinal anchor + 7-field scoring."""
    season = sh.season_ordinal
    season_bytes = struct.pack("<i", season)
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
                idx = data.find(season_bytes, pos)
                if idx == -1:
                    break
                hits.append(region.base + offset + idx)
                pos = idx + 1
            offset += read_size

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
            k = struct.unpack_from("<Q", klass_data, 0)[0]
            m = struct.unpack_from("<Q", klass_data, 8)[0]
            if (0x1_0000_0000 <= k <= 0x7FFF_FFFF_FFFF) and m == 0:
                klass = k

        if score >= 7 and klass is not None:
            # Check parent count to disambiguate duplicates
            obj_addr = addr - 16
            parent_count = _count_parents(pm, regions, obj_addr)
            scored.append({
                "address": obj_addr,
                "scalars_addr": addr,
                "klass": klass,
                "score": score,
                "parent_count": parent_count,
                "fields": {
                    "con_season": vals[0], "con_unknown": vals[1],
                    "con_level": vals[2], "con_step": vals[3],
                    "con_wins": vals[4], "con_draws": vals[5], "con_losses": vals[6],
                    "ltd_season": vals[7], "ltd_unknown": vals[8],
                    "ltd_level": vals[9], "ltd_step": vals[10],
                    "ltd_wins": vals[11], "ltd_losses": vals[12],
                },
            })

    if not scored:
        return None

    # Prefer candidate with parents (real object, not orphan copy)
    scored.sort(key=lambda c: (-c["parent_count"], -c["score"]))
    best = scored[0]
    best["valid"] = best["parent_count"] > 0
    best["total_candidates"] = len(scored)
    return best


def _count_parents(pm, regions, obj_addr: int, limit: int = 5) -> int:
    """Count pointers to an object address (capped at limit for speed)."""
    target_bytes = struct.pack("<Q", obj_addr)
    chunk_size = 4 * 1024 * 1024
    count = 0

    for region in regions:
        offset = 0
        while offset < region.size and count < limit:
            read_size = min(chunk_size, region.size - offset)
            if read_size < 8:
                offset += read_size
                continue
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
                count += 1
                if count >= limit:
                    break
                pos = idx + 1
            offset += read_size
        if count >= limit:
            break

    return count


# ── Target 7: Mastery LevelTrack ────────────────────────────────────

def discover_mastery(pm, regions, progress: int) -> dict | None:
    """Find Mastery LevelTrack nodes via progress value anchor."""
    if progress <= 0:
        return None

    progress_bytes = struct.pack("<i", progress)
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
                idx = data.find(progress_bytes, pos)
                if idx == -1:
                    break
                hits.append(region.base + offset + idx)
                pos = idx + 1
            offset += read_size

    # Validate each hit
    candidates = []
    for addr in hits:
        element_base = addr - 48
        data = read_memory(pm, element_base - 64, 128)
        if data is None:
            continue

        off = 64  # current element starts at offset 64 in our read
        klass = struct.unpack_from("<Q", data, off)[0]
        monitor = struct.unpack_from("<Q", data, off + 8)[0]
        ptr_a = struct.unpack_from("<Q", data, off + 16)[0]
        ptr_c = struct.unpack_from("<Q", data, off + 32)[0]
        prev_level = struct.unpack_from("<i", data, off + 40)[0]
        level_num = struct.unpack_from("<i", data, off + 44)[0]
        curr_progress = struct.unpack_from("<i", data, off + 48)[0]

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

        score = 0
        # Sibling klass match
        sib_klass = struct.unpack_from("<Q", data, 0)[0]
        if sib_klass == klass:
            score += 2
        # XP requirement
        xp_data = read_memory(pm, ptr_a, 24)
        if xp_data:
            xp_val = struct.unpack_from("<i", xp_data, 16)[0]
            if xp_val == 1000:
                score += 2

        if score >= 4:
            name = read_il2cpp_string(pm, ptr_c)
            candidates.append({
                "element_base": element_base,
                "level": level_num,
                "progress": curr_progress,
                "klass": klass,
                "name": name,
                "score": score,
            })

    if not candidates:
        return None

    best = candidates[0]

    # Count adjacent levels
    adj_levels = []
    for delta in range(-5, 6):
        if delta == 0:
            continue
        adj_addr = best["element_base"] + delta * 64
        adj_data = read_memory(pm, adj_addr, 64)
        if adj_data:
            adj_klass = struct.unpack_from("<Q", adj_data, 0)[0]
            if adj_klass == best["klass"]:
                adj_level = struct.unpack_from("<i", adj_data, 44)[0]
                adj_prog = struct.unpack_from("<i", adj_data, 48)[0]
                adj_levels.append({"level": adj_level, "progress": adj_prog})

    best["adjacent_levels"] = adj_levels
    best["valid"] = True
    best["total_candidates"] = len(candidates)
    return best


# ── Main ─────────────────────────────────────────────────────────────

def main() -> int:
    parser = argparse.ArgumentParser(description="Full discovery of all known memory targets")
    parser.add_argument("--progress", type=int, default=575,
                        help="Current mastery XP progress (default: 575 from last session)")
    parser.add_argument("--json", action="store_true",
                        help="Output results as JSON only")
    args = parser.parse_args()

    sh = extract_starthook()

    if not args.json:
        print("Full Memory Target Discovery")
        print("=" * 60)

    pm = open_mtga()
    if not args.json:
        print(f"  PID: {pm.process_id}")

    t0 = time.perf_counter()
    regions = enumerate_regions(pm)
    t_enum = time.perf_counter() - t0
    if not args.json:
        print(f"  {len(regions)} regions ({t_enum:.1f}s)")

    results = {}

    # ── 1. Inventory ──
    t1 = time.perf_counter()
    inv = discover_inventory(pm, regions, sh)
    t_inv = time.perf_counter() - t1
    results["inventory"] = inv
    if not args.json:
        if inv:
            print(f"\n[1] Inventory: {inv['inv_addr']:#x} (score={inv['score']}) [{t_inv:.1f}s]")
        else:
            print(f"\n[1] Inventory: NOT FOUND [{t_inv:.1f}s]")

    if inv is None:
        if not args.json:
            print("Cannot proceed without inventory. Aborting.")
        pm.close_process()
        return 1

    inv_addr = inv["inv_addr"]

    # ── 2. CustomTokens (navigation from inventory) ──
    t2 = time.perf_counter()
    tokens = discover_custom_tokens(pm, inv_addr, sh)
    t_tok = time.perf_counter() - t2
    results["custom_tokens"] = tokens
    if not args.json:
        if tokens:
            valid = "VALID" if tokens["valid"] else "MISMATCH"
            print(f"[2] CustomTokens: {tokens['address']:#x} (count={tokens['count']}, {valid}) [{t_tok:.3f}s]")
            for k, v in tokens["tokens"].items():
                print(f"      {k} = {v}")
        else:
            print(f"[2] CustomTokens: NOT FOUND [{t_tok:.3f}s]")

    # ── 3. Boosters (navigation from inventory) ──
    t3 = time.perf_counter()
    boosters = discover_boosters(pm, inv_addr, sh)
    t_boo = time.perf_counter() - t3
    results["boosters"] = boosters
    if not args.json:
        if boosters:
            valid = "VALID" if boosters["valid"] else "MISMATCH"
            print(f"[3] Boosters: {boosters['list_address']:#x} (size={boosters['size']}, {valid}) [{t_boo:.3f}s]")
            for b in boosters["boosters"]:
                print(f"      CollationId={b['collation_id']}, Count={b['count']}")
        else:
            print(f"[3] Boosters: NOT FOUND [{t_boo:.3f}s]")

    # ── 4. Report Queue (pointer search) ──
    t4 = time.perf_counter()
    queue = discover_report_queue(pm, regions, inv["inv_obj_base"])
    t_q = time.perf_counter() - t4
    results["report_queue"] = queue
    if not args.json:
        if queue:
            valid = "validated" if queue.get("validated") else "unvalidated"
            print(f"[4] ReportQueue: InventoryClient={queue['inv_client_addr']:#x}, "
                  f"queue_size={queue['queue_size']}, version={queue['queue_version']} "
                  f"({valid}) [{t_q:.1f}s]")
        else:
            print(f"[4] ReportQueue: NOT FOUND [{t_q:.1f}s]")

    # ── 5. CosmeticsClient (global scan) ──
    t5 = time.perf_counter()
    cosmetics = discover_cosmetics(pm, regions, sh)
    t_cos = time.perf_counter() - t5
    results["cosmetics"] = cosmetics
    if not args.json:
        if cosmetics:
            valid = "VALID" if cosmetics["valid"] else "AMBIGUOUS"
            print(f"[5] Cosmetics: {cosmetics['address']:#x} ({valid}) [{t_cos:.1f}s]")
            for cat, count in cosmetics["sizes"].items():
                print(f"      {cat}: {count}")
        else:
            print(f"[5] Cosmetics: NOT FOUND [{t_cos:.1f}s]")

    # ── 6. Rank State (global scan) ──
    t6 = time.perf_counter()
    rank = discover_rank(pm, regions, sh)
    t_rank = time.perf_counter() - t6
    results["rank"] = rank
    if not args.json:
        if rank:
            valid = "VALID" if rank["valid"] else "ORPHAN"
            print(f"[6] Rank: {rank['address']:#x} (score={rank['score']}/8, "
                  f"parents={rank['parent_count']}, {valid}) [{t_rank:.1f}s]")
            f = rank["fields"]
            print(f"      Constructed: level={f['con_level']}, step={f['con_step']}, "
                  f"wins={f['con_wins']}")
            print(f"      Limited:     level={f['ltd_level']}, step={f['ltd_step']}, "
                  f"wins={f['ltd_wins']}, losses={f['ltd_losses']}")
        else:
            print(f"[6] Rank: NOT FOUND [{t_rank:.1f}s]")

    # ── 7. Mastery LevelTrack (global scan) ──
    t7 = time.perf_counter()
    mastery = discover_mastery(pm, regions, args.progress)
    t_mast = time.perf_counter() - t7
    results["mastery"] = mastery
    if not args.json:
        if mastery:
            print(f"[7] Mastery: level={mastery['level']}, progress={mastery['progress']}, "
                  f"score={mastery['score']}/4 [{t_mast:.1f}s]")
            print(f"      Name: {mastery['name']}")
            print(f"      Adjacent: {[a['level'] for a in mastery['adjacent_levels']]}")
        else:
            print(f"[7] Mastery: NOT FOUND [{t_mast:.1f}s]")

    t_total = time.perf_counter() - t0

    if not args.json:
        # Summary
        found = sum(1 for v in results.values() if v is not None)
        valid = sum(1 for v in results.values() if v and v.get("valid", False))
        print(f"\n{'=' * 60}")
        print(f"Discovery complete: {found}/7 found, {valid}/7 valid ({t_total:.1f}s total)")

        # Timing breakdown
        print(f"\nTiming: enum={t_enum:.1f}s, inv={t_inv:.1f}s, "
              f"tokens={t_tok:.3f}s, boosters={t_boo:.3f}s, "
              f"queue={t_q:.1f}s, cosmetics={t_cos:.1f}s, "
              f"rank={t_rank:.1f}s, mastery={t_mast:.1f}s")

    if args.json:
        # Convert addresses to hex strings for JSON
        def hexify(obj):
            if isinstance(obj, dict):
                return {k: hexify(v) for k, v in obj.items()}
            if isinstance(obj, list):
                return [hexify(v) for v in obj]
            if isinstance(obj, int) and obj > 0xFFFF:
                return f"0x{obj:x}"
            return obj

        print(json.dumps(hexify(results), indent=2))

    pm.close_process()
    return 0


if __name__ == "__main__":
    sys.exit(main())
