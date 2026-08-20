"""Shared readers for the match session data vacuum.

Memory readers, log tailing, debug server access, and collection snapshots.
Used by match_session.py — extracted to keep the orchestrator lean.
"""
from __future__ import annotations

import json
import struct
import time
import urllib.request
from datetime import datetime
from pathlib import Path

from .probe import read_i32, read_i64, read_memory
from .verify import read_il2cpp_string
from .delta_mapper import (
    read_queue_state,
    read_report_item,
    read_context_source_type,
    dump_inventory_delta,
    read_aetherized_cards,
    SOURCE_NAMES,
)


def ts() -> str:
    return datetime.now().strftime("%H:%M:%S.%f")[:-3]


def ts_full() -> str:
    return datetime.now().isoformat()


# ── Memory readers ───────────────────────────────────────────────────


def read_inventory_scalars(pm, addr: int) -> dict[str, int | float] | None:
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
        "vault_raw": struct.unpack_from("<d", data, 32)[0],
    }


def read_rank_state(pm, addr: int) -> dict[str, int] | None:
    data = read_memory(pm, addr, 52)
    if data is None:
        return None
    vals = [struct.unpack_from("<i", data, i)[0] for i in range(0, 52, 4)]
    return {
        "con_season": vals[0], "con_class": vals[1],
        "con_level": vals[2], "con_step": vals[3],
        "con_wins": vals[4], "con_draws": vals[5], "con_losses": vals[6],
        "ltd_season": vals[7], "ltd_class": vals[8],
        "ltd_level": vals[9], "ltd_step": vals[10],
        "ltd_wins": vals[11], "ltd_losses": vals[12],
    }


def read_boosters(pm, inv_addr: int) -> list[dict] | None:
    list_ptr = read_i64(pm, inv_addr - 64)
    if not list_ptr or not (0x1_0000_0000 <= list_ptr <= 0x7FFF_FFFF_FFFF):
        return None
    list_data = read_memory(pm, list_ptr, 32)
    if list_data is None:
        return None
    items_ptr = struct.unpack_from("<Q", list_data, 16)[0]
    size = struct.unpack_from("<i", list_data, 24)[0]
    if size < 0 or size > 100:
        return None
    boosters = []
    if items_ptr and (0x1_0000_0000 <= items_ptr <= 0x7FFF_FFFF_FFFF):
        arr_data = read_memory(pm, items_ptr, 32 + size * 8)
        if arr_data:
            for i in range(size):
                elem_ptr = struct.unpack_from("<Q", arr_data, 32 + i * 8)[0]
                if not elem_ptr or not (0x1_0000_0000 <= elem_ptr <= 0x7FFF_FFFF_FFFF):
                    continue
                bs_data = read_memory(pm, elem_ptr, 32)
                if bs_data:
                    boosters.append({
                        "collation_id": struct.unpack_from("<i", bs_data, 16)[0],
                        "count": struct.unpack_from("<i", bs_data, 20)[0],
                    })
    return boosters


def read_custom_tokens(pm, inv_addr: int) -> dict[str, int] | None:
    dict_ptr = read_i64(pm, inv_addr - 8)
    if not dict_ptr or not (0x1_0000_0000 <= dict_ptr <= 0x7FFF_FFFF_FFFF):
        return None
    dict_data = read_memory(pm, dict_ptr, 80)
    if dict_data is None:
        return None
    entries_ptr = struct.unpack_from("<Q", dict_data, 24)[0]
    count = struct.unpack_from("<i", dict_data, 64)[0]
    if count < 0 or count > 100:
        return None
    tokens = {}
    if entries_ptr and (0x1_0000_0000 <= entries_ptr <= 0x7FFF_FFFF_FFFF):
        arr_header = read_memory(pm, entries_ptr, 32)
        if arr_header:
            max_length = struct.unpack_from("<Q", arr_header, 24)[0]
            if max_length < 1000:
                entries_data = read_memory(pm, entries_ptr + 32, min(max_length, 100) * 24)
                if entries_data:
                    for i in range(min(count, max_length)):
                        off = i * 24
                        if off + 24 > len(entries_data):
                            break
                        hash_code = struct.unpack_from("<i", entries_data, off)[0]
                        key_ptr = struct.unpack_from("<Q", entries_data, off + 8)[0]
                        value = struct.unpack_from("<i", entries_data, off + 16)[0]
                        if hash_code < 0:
                            continue
                        if key_ptr and (0x1_0000_0000 <= key_ptr <= 0x7FFF_FFFF_FFFF):
                            key_name = read_il2cpp_string(pm, key_ptr)
                            if key_name:
                                tokens[key_name] = value
    return tokens


def read_mastery_node(pm, element_base: int) -> dict | None:
    data = read_memory(pm, element_base, 64)
    if data is None:
        return None
    klass = struct.unpack_from("<Q", data, 0)[0]
    if not (0x1_0000_0000 <= klass <= 0x7FFF_FFFF_FFFF):
        return None
    monitor = struct.unpack_from("<Q", data, 8)[0]
    if monitor != 0:
        return None
    level = struct.unpack_from("<i", data, 44)[0]
    progress = struct.unpack_from("<i", data, 48)[0]
    prev = struct.unpack_from("<i", data, 40)[0]
    if not (1 <= level <= 200) or prev != level - 1:
        return None
    return {"level": level, "progress": progress, "klass": klass}


def read_cosmetics_sizes(pm, cosm_addr: int) -> dict[str, int] | None:
    """Read 6 list sizes from a known CosmeticsClient address."""
    categories = ["art_styles", "avatars", "pets", "sleeves", "emotes", "titles"]
    sizes = {}
    for j, name in enumerate(categories):
        ptr = read_i64(pm, cosm_addr + 16 + j * 8)
        if not ptr or not (0x1_0000_0000 <= ptr <= 0x7FFF_FFFF_FFFF):
            return None
        list_data = read_memory(pm, ptr, 32)
        if list_data is None:
            return None
        sizes[name] = struct.unpack_from("<i", list_data, 24)[0]
    return sizes


# ── Rank finder ──────────────────────────────────────────────────────


def find_rank_address(pm, regions, sh) -> int | None:
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

    best = None
    best_score = 0
    for addr in hits:
        data = read_memory(pm, addr, 52)
        if data is None:
            continue
        vals = [struct.unpack_from("<i", data, i)[0] for i in range(0, 52, 4)]
        if len(vals) < 13:
            continue
        score = sum([
            vals[0] == season, vals[7] == season,
            vals[2] == sh.constructed_level, vals[3] == sh.constructed_step,
            vals[4] == sh.constructed_wins,
            vals[9] == sh.limited_level, vals[10] == sh.limited_step,
            vals[11] == sh.limited_wins,
        ])
        klass_data = read_memory(pm, addr - 16, 16)
        if klass_data is None:
            continue
        k = struct.unpack_from("<Q", klass_data, 0)[0]
        m = struct.unpack_from("<Q", klass_data, 8)[0]
        if not (0x1_0000_0000 <= k <= 0x7FFF_FFFF_FFFF) or m != 0:
            continue
        if score >= 7 and score > best_score:
            best = addr
            best_score = score
    return best


# ── Mastery finder ───────────────────────────────────────────────────


def find_mastery_address(pm, regions, progress: int) -> int | None:
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

    for addr in hits:
        element_base = addr - 48
        node = read_mastery_node(pm, element_base)
        if node is None:
            continue
        sib_data = read_memory(pm, element_base - 64, 8)
        if sib_data:
            sib_klass = struct.unpack_from("<Q", sib_data, 0)[0]
            if sib_klass != node["klass"]:
                continue
        ptr_a = read_i64(pm, element_base + 16)
        if ptr_a:
            xp_data = read_memory(pm, ptr_a, 24)
            if xp_data:
                xp = struct.unpack_from("<i", xp_data, 16)[0]
                if xp != 1000:
                    continue
        return element_base

    return None


# ── Debug server ─────────────────────────────────────────────────────


def fetch_debug_server(endpoint: str = "/api/dump") -> dict | None:
    try:
        url = f"http://127.0.0.1:9876{endpoint}"
        req = urllib.request.Request(url, headers={"Accept": "application/json"})
        with urllib.request.urlopen(req, timeout=5) as resp:
            return json.loads(resp.read())
    except Exception as e:
        print(f"  [debug server] {endpoint} failed: {e}")
        return None


# ── Player.log ───────────────────────────────────────────────────────


LOG_PATH = Path.home() / "AppData" / "LocalLow" / "Wizards Of The Coast" / "MTGA" / "Player.log"


def get_log_size() -> int:
    return LOG_PATH.stat().st_size if LOG_PATH.exists() else 0


def read_new_log_lines(start_offset: int) -> list[str]:
    if not LOG_PATH.exists():
        return []
    try:
        with open(LOG_PATH, "r", encoding="utf-8", errors="replace") as f:
            f.seek(start_offset)
            return f.readlines()
    except Exception:
        return []


def parse_log_json_objects(lines: list[str], max_size: int = 100000) -> list[dict]:
    """Parse JSON objects from log lines. Keeps everything under max_size bytes."""
    objects = []
    for line in lines:
        stripped = line.strip()
        if stripped.startswith("{"):
            try:
                obj = json.loads(stripped)
                if len(stripped) < max_size:
                    objects.append(obj)
            except json.JSONDecodeError:
                pass
    return objects


# ── Report item capture ──────────────────────────────────────────────


def capture_report_item(pm, item_ptr: int) -> dict | None:
    """Capture a report item with ALL data including raw delta bytes."""
    item = read_report_item(pm, item_ptr)
    if item is None:
        return None

    result = {
        "address": item_ptr,
        "klass": item["klass"],
        "xp_gained": item["xp_gained"],
        "source_type": None,
        "source_name": None,
        "parent_context": None,
        "delta": None,
        "delta_raw_bytes": None,
        "delta_raw_values": None,
        "aetherized_cards": [],
    }

    source_type = read_context_source_type(pm, item["context_ptr"])
    result["source_type"] = source_type
    result["source_name"] = SOURCE_NAMES.get(source_type, f"unknown({source_type})")

    if item["parent_ptr"] and (0x1_0000_0000 <= item["parent_ptr"] <= 0x7FFF_FFFF_FFFF):
        result["parent_context"] = read_il2cpp_string(pm, item["parent_ptr"])

    # Full InventoryDelta — raw dump ALL 160 bytes
    delta = dump_inventory_delta(pm, item["delta_ptr"])
    if delta:
        # Raw bytes of entire delta object for post-analysis
        raw_full = read_memory(pm, item["delta_ptr"], 160)
        if raw_full:
            result["delta_raw_bytes"] = raw_full.hex()

        # Value section (+104 to +159)
        raw_values = {}
        for off, val in delta["value_i32s"].items():
            if 104 <= off <= 159:
                raw_values[str(off)] = val
        result["delta_raw_values"] = raw_values

        # Reference field arrays — read actual contents
        ref_names = [
            "boosterDelta", "cardsAdded", "decksAdded",
            "vanityAdded", "vanityRemoved", "artSkinsAdded",
            "artSkinsRemoved", "vouchersDelta", "tickets",
            "customTokenDelta", "newLetters"
        ]
        refs = {}
        for j, (ptr, name) in enumerate(zip(delta["ref_ptrs"], ref_names)):
            if ptr == 0:
                refs[name] = None
            elif 0x1_0000_0000 <= ptr <= 0x7FFF_FFFF_FFFF:
                arr_info = _read_array_or_list(pm, ptr, name)
                refs[name] = arr_info
            else:
                refs[name] = {"ptr": ptr, "type": "not_pointer"}
        result["delta"] = refs

    result["aetherized_cards"] = read_aetherized_cards(pm, item["aetherized_ptr"])

    return result


def _read_array_or_list(pm, ptr: int, name: str) -> dict:
    """Read array/list pointed to by an InventoryDelta reference field.
    Dumps raw element data for post-analysis."""
    target = read_memory(pm, ptr, 32)
    if not target:
        return {"ptr": ptr, "type": "unreadable"}

    klass = struct.unpack_from("<Q", target, 0)[0]
    max_length = struct.unpack_from("<Q", target, 24)[0]

    # IL2CPP array: klass(8) + monitor(8) + bounds(8) + max_length(8) + elements...
    if max_length < 500:
        info = {"ptr": ptr, "type": "array", "length": max_length}
        # For int arrays (cardsAdded), read all elements
        if name in ("cardsAdded", "boosterDelta") and max_length > 0:
            elem_data = read_memory(pm, ptr + 32, min(max_length, 200) * 4)
            if elem_data:
                elements = []
                for i in range(min(max_length, 200)):
                    elements.append(struct.unpack_from("<i", elem_data, i * 4)[0])
                info["elements_i32"] = elements
        # For pointer arrays (vanityAdded, artSkinsAdded, customTokenDelta, etc.)
        elif max_length > 0 and max_length <= 50:
            elem_data = read_memory(pm, ptr + 32, max_length * 8)
            if elem_data:
                ptrs = []
                for i in range(max_length):
                    p = struct.unpack_from("<Q", elem_data, i * 8)[0]
                    ptrs.append(p)
                info["elements_ptr"] = ptrs
                # Try reading raw 64 bytes of first few elements for post-analysis
                raw_elems = []
                for p in ptrs[:5]:
                    if p and (0x1_0000_0000 <= p <= 0x7FFF_FFFF_FFFF):
                        raw = read_memory(pm, p, 64)
                        if raw:
                            raw_elems.append({"ptr": p, "hex": raw.hex()})
                if raw_elems:
                    info["element_dumps"] = raw_elems
        return info

    # Might be a List<T>: klass(8) + monitor(8) + _items(8) + _size(4) + _version(4)
    size_at_24 = struct.unpack_from("<i", target, 24)[0]
    if 0 <= size_at_24 <= 1000:
        return {"ptr": ptr, "type": "list", "size": size_at_24}

    return {"ptr": ptr, "type": "unknown", "klass": klass}


# ── Collection snapshot ──────────────────────────────────────────────


def snapshot_collection_summary(pm, regions) -> dict | None:
    """Quick collection read — returns {total_unique, total_copies} without
    needing the card database. Scans for the Dict<int,int> directly."""
    from .probe import find_inventory
    # We can't do a full scan_collection without the card DB.
    # Instead, just report what the debug server has.
    ds = fetch_debug_server("/api/dump")
    if ds and "collection_summary" in ds:
        return ds["collection_summary"]
    return None


# ── Persistence ──────────────────────────────────────────────────────


def save_session(path: Path, data: dict) -> None:
    """Save session data to JSON, converting non-serializable types."""
    def default(obj):
        if isinstance(obj, (bytes, bytearray)):
            return obj.hex()
        if isinstance(obj, set):
            return list(obj)
        return str(obj)

    with open(path, "w") as f:
        json.dump(data, f, indent=2, default=default)
