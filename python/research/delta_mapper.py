"""Map InventoryDelta value field ordering by watching the report queue.

Finds the report queue via: inventory → inv_obj_base → InventoryClient → +112 → queue.
Monitors queue version. When a new ReportItem appears, dumps the full InventoryDelta
value section alongside known inventory scalar deltas for field identification.

Run from mtga-tool/: python -m python.research.delta_mapper [--gold N]
"""
from __future__ import annotations

import argparse
import struct
import sys
import time

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


# ── Helpers ──────────────────────────────────────────────────────────

def read_inventory_scalars(pm, addr: int) -> dict[str, int | float] | None:
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
        "vault_progress": struct.unpack_from("<d", data, 32)[0],
    }


def find_inventory_client(pm, regions, inv_obj_base: int) -> int | None:
    """Find the InventoryClient parent object by searching for pointers to inv_obj_base.

    InventoryClient has inv_obj_base at +80 and report queue List at +112.
    Returns the InventoryClient object address, or None.
    """
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
                # Check if this pointer is at +80 of a valid IL2CPP object
                obj_base = ptr_addr - 80
                header = read_memory(pm, obj_base, 16)
                if header is not None:
                    klass = struct.unpack_from("<Q", header, 0)[0]
                    monitor = struct.unpack_from("<Q", header, 8)[0]
                    if (0x1_0000_0000 <= klass <= 0x7FFF_FFFF_FFFF) and monitor == 0:
                        # Check +112 for a valid List pointer
                        list_ptr = read_i64(pm, obj_base + 112)
                        if list_ptr and (0x1_0000_0000 <= list_ptr <= 0x7FFF_FFFF_FFFF):
                            # Check if target at list_ptr has valid List layout
                            list_data = read_memory(pm, list_ptr, 32)
                            if list_data:
                                list_klass = struct.unpack_from("<Q", list_data, 0)[0]
                                list_size = struct.unpack_from("<i", list_data, 24)[0]
                                if (0x1_0000_0000 <= list_klass <= 0x7FFF_FFFF_FFFF) and 0 <= list_size <= 100:
                                    candidates.append((obj_base, klass, list_ptr, list_size))
                pos = idx + 1
            offset += read_size

    if not candidates:
        return None

    # Prefer the candidate whose queue List has elements with ReportItem klass
    # (if queue has items) — otherwise take the first valid one
    for obj_base, klass, list_ptr, list_size in candidates:
        if list_size > 0:
            # Read _items array pointer
            items_ptr = read_i64(pm, list_ptr + 16)
            if items_ptr and (0x1_0000_0000 <= items_ptr <= 0x7FFF_FFFF_FFFF):
                # Read first element pointer (array header = 32 bytes)
                elem_ptr = read_i64(pm, items_ptr + 32)
                if elem_ptr and (0x1_0000_0000 <= elem_ptr <= 0x7FFF_FFFF_FFFF):
                    # Valid element — this is likely the right candidate
                    print(f"  InventoryClient at {obj_base:#x} (klass={klass:#x}, queue_size={list_size})")
                    return obj_base

    # Fall back to first candidate
    obj_base, klass, list_ptr, list_size = candidates[0]
    print(f"  InventoryClient at {obj_base:#x} (klass={klass:#x}, queue_size={list_size})")
    return obj_base


def read_queue_state(pm, queue_list_ptr: int) -> tuple[int, int, int] | None:
    """Read queue List state: (_items_ptr, _size, _version)."""
    data = read_memory(pm, queue_list_ptr, 32)
    if data is None:
        return None
    items_ptr = struct.unpack_from("<Q", data, 16)[0]
    size = struct.unpack_from("<i", data, 24)[0]
    version = struct.unpack_from("<i", data, 28)[0]
    return (items_ptr, size, version)


def read_report_item(pm, item_ptr: int) -> dict | None:
    """Read a ClientInventoryUpdateReportItem (56 bytes).

    Layout:
        +0:  klass (8)
        +8:  monitor (8, null)
        +16: delta ptr (InventoryDelta)
        +24: aetherizedCards ptr (List<AetherizedCardInformation>)
        +32: context ptr (InventoryUpdateContext)
        +40: parentcontext ptr (String, may be null)
        +48: xpGained (i32)
        +52: padding (i32)
    """
    data = read_memory(pm, item_ptr, 56)
    if data is None:
        return None

    klass = struct.unpack_from("<Q", data, 0)[0]
    delta_ptr = struct.unpack_from("<Q", data, 16)[0]
    aether_ptr = struct.unpack_from("<Q", data, 24)[0]
    context_ptr = struct.unpack_from("<Q", data, 32)[0]
    parent_ptr = struct.unpack_from("<Q", data, 40)[0]
    xp_gained = struct.unpack_from("<i", data, 48)[0]

    return {
        "klass": klass,
        "delta_ptr": delta_ptr,
        "aetherized_ptr": aether_ptr,
        "context_ptr": context_ptr,
        "parent_ptr": parent_ptr,
        "xp_gained": xp_gained,
    }


def read_context_source_type(pm, context_ptr: int) -> int | None:
    """Read InventoryUpdateContext._sourceType at +24."""
    if context_ptr == 0 or not (0x1_0000_0000 <= context_ptr <= 0x7FFF_FFFF_FFFF):
        return None
    return read_i32(pm, context_ptr + 24)


def dump_inventory_delta(pm, delta_ptr: int) -> dict | None:
    """Dump the full InventoryDelta object — reference fields and ALL value fields.

    Known layout:
        +0:   klass (8)
        +8:   monitor (8, null)
        +16 to +96: 11 reference fields (pointers)
        +104+: value fields (i32/f64 — what we're mapping)

    We read 256 bytes to capture everything.
    """
    if delta_ptr == 0 or not (0x1_0000_0000 <= delta_ptr <= 0x7FFF_FFFF_FFFF):
        return None

    data = read_memory(pm, delta_ptr, 256)
    if data is None:
        return None

    result = {
        "address": delta_ptr,
        "klass": struct.unpack_from("<Q", data, 0)[0],
    }

    # Reference fields at +16 to +96
    refs = []
    for off in range(16, 104, 8):
        ptr = struct.unpack_from("<Q", data, off)[0]
        refs.append(ptr)
    result["ref_ptrs"] = refs

    # Value fields from +104 onward — dump as i32 values
    value_i32s = {}
    for off in range(104, 256, 4):
        val = struct.unpack_from("<i", data, off)[0]
        value_i32s[off] = val
    result["value_i32s"] = value_i32s

    # Also try reading as f64 pairs for vault_progress_delta (might be f64)
    value_f64s = {}
    for off in range(104, 248, 8):
        val = struct.unpack_from("<d", data, off)[0]
        value_f64s[off] = val
    result["value_f64s"] = value_f64s

    return result


def read_aetherized_cards(pm, list_ptr: int) -> list[dict]:
    """Read List<AetherizedCardInformation> and return card details."""
    if list_ptr == 0 or not (0x1_0000_0000 <= list_ptr <= 0x7FFF_FFFF_FFFF):
        return []

    list_data = read_memory(pm, list_ptr, 32)
    if list_data is None:
        return []

    items_ptr = struct.unpack_from("<Q", list_data, 16)[0]
    size = struct.unpack_from("<i", list_data, 24)[0]

    if size <= 0 or size > 100 or items_ptr == 0:
        return []

    # Read array: header(32) + size * 8 (pointers)
    arr_data = read_memory(pm, items_ptr, 32 + size * 8)
    if arr_data is None:
        return []

    cards = []
    for i in range(size):
        elem_ptr = struct.unpack_from("<Q", arr_data, 32 + i * 8)[0]
        if elem_ptr == 0 or not (0x1_0000_0000 <= elem_ptr <= 0x7FFF_FFFF_FFFF):
            continue

        card_data = read_memory(pm, elem_ptr, 48)
        if card_data is None:
            continue

        set_ptr = struct.unpack_from("<Q", card_data, 16)[0]
        grp_id = struct.unpack_from("<i", card_data, 24)[0]
        added = struct.unpack_from("<i", card_data, 28)[0]
        from_deck = struct.unpack_from("<i", card_data, 32)[0]
        vault_prog = struct.unpack_from("<f", card_data, 36)[0]
        gold_awarded = struct.unpack_from("<i", card_data, 40)[0]
        gems_awarded = struct.unpack_from("<i", card_data, 44)[0]

        set_name = None
        if set_ptr and (0x1_0000_0000 <= set_ptr <= 0x7FFF_FFFF_FFFF):
            set_name = read_il2cpp_string(pm, set_ptr)

        cards.append({
            "grpId": grp_id,
            "set": set_name,
            "addedToInventory": added,
            "isGrantedFromDeck": from_deck,
            "vaultProgress": vault_prog,
            "goldAwarded": gold_awarded,
            "gemsAwarded": gems_awarded,
        })

    return cards


# ── InventoryUpdateSource enum (client-side) ────────────────────────

SOURCE_NAMES = {
    0: "None",
    1: "Booster",
    2: "WildCardRedemption",
    3: "CatalogPurchase",
    4: "EventReward",
    5: "QuestReward",
    6: "PlayerReward",
    7: "SeasonReward",
    8: "StartupGranted",
    9: "IsGrantedFromDeck",
    10: "EventPayEntry",
    11: "EventEntryReward",
    12: "MercantilePurchase",
    13: "CodeRedemption",
    14: "CampaignGraphReward",
    15: "VaultReward",
    16: "ChestReward",
    17: "DraftPick",
    18: "CatchUp",
    19: "TopDecksReward",
    20: "RenewalReward",
    21: "MercantileBoosterPurchase",
    22: "BattlePassLevelUp",
    23: "BoosterOpen",
    24: "MercantileChestPurchase",
    25: "ProgressionRewardTier",
    26: "BattlePassLevelMasteryTree",
    27: "CampaignGraphAutomaticPayoutNode",
    28: "CampaignGraphTieredRewardNode",
    29: "InboxClaim",
    30: "ICEReward",
    # ... more may exist
}


# ── Main ─────────────────────────────────────────────────────────────

def main() -> int:
    parser = argparse.ArgumentParser(description="Map InventoryDelta value fields via report queue")
    parser.add_argument("--gold", type=int, default=None,
                        help="Override gold value for inventory search (if changed since StartHook)")
    args = parser.parse_args()

    sh = extract_starthook()
    overrides = {}
    if args.gold is not None:
        overrides["gold"] = args.gold

    # Build anchor values, excluding gold if overridden
    known = {
        "gold": overrides.get("gold", sh.gold),
        "gems": sh.gems,
        "wc_common": sh.wc_common,
        "wc_uncommon": sh.wc_uncommon,
        "wc_rare": sh.wc_rare,
        "wc_mythic": sh.wc_mythic,
        "wc_track_position": sh.wc_track_position,
    }
    known = {k: v for k, v in known.items() if v != 0}

    if len(known) < 3:
        print(f"ERROR: Only {len(known)} non-zero anchor values. Need >= 3.", file=sys.stderr)
        return 1

    print(f"Opening MTGA.exe...")
    pm = open_mtga()
    print(f"  PID: {pm.process_id}")

    print(f"Enumerating memory regions...")
    regions = enumerate_regions(pm)
    print(f"  {len(regions)} readable regions")

    print(f"Finding inventory ({len(known)} anchors)...")
    location = find_inventory(pm, regions, known)
    if location is None:
        print("ERROR: Inventory not found.", file=sys.stderr)
        pm.close_process()
        return 1

    inv_addr = location.address
    inv_obj_base = inv_addr - 80
    print(f"  Inventory scalars at {inv_addr:#x}")
    print(f"  Inventory object base at {inv_obj_base:#x}")

    # ── Find InventoryClient parent ──
    print(f"\nSearching for InventoryClient (pointers to inv_obj_base)...")
    inv_client = find_inventory_client(pm, regions, inv_obj_base)
    if inv_client is None:
        print("ERROR: InventoryClient not found.", file=sys.stderr)
        pm.close_process()
        return 1

    queue_list_ptr = read_i64(pm, inv_client + 112)
    if queue_list_ptr is None or queue_list_ptr == 0:
        print("ERROR: Queue list pointer at InventoryClient+112 is null.", file=sys.stderr)
        pm.close_process()
        return 1

    print(f"  Queue List at {queue_list_ptr:#x}")

    # Read initial queue state
    q_state = read_queue_state(pm, queue_list_ptr)
    if q_state is None:
        print("ERROR: Failed to read queue state.", file=sys.stderr)
        pm.close_process()
        return 1

    items_ptr, q_size, q_version = q_state
    print(f"  Queue: _size={q_size}, _version={q_version}, _items={items_ptr:#x}")

    # Read current inventory scalars
    before = read_inventory_scalars(pm, inv_addr)
    if before is None:
        print("ERROR: Failed to read inventory scalars.", file=sys.stderr)
        pm.close_process()
        return 1

    print(f"\nCurrent inventory state:")
    for k, v in before.items():
        if k == "vault_progress":
            print(f"  {k:20s} = {v:.4f} ({v * 10:.1f} raw)")
        else:
            print(f"  {k:20s} = {v}")

    # ── Watch for queue changes ──
    print(f"\n{'=' * 60}")
    print(f"WATCHING report queue for new entries...")
    print(f"Perform an in-game action NOW (claim quest, open pack, win game, etc.)")
    print(f"Press Ctrl+C to stop.")
    print(f"{'=' * 60}")

    last_version = q_version
    last_size = q_size
    poll_count = 0

    try:
        while True:
            time.sleep(0.05)  # 50ms polling
            poll_count += 1

            state = read_queue_state(pm, queue_list_ptr)
            if state is None:
                continue

            curr_items_ptr, curr_size, curr_version = state

            if curr_version != last_version:
                elapsed = poll_count * 0.05
                print(f"\n*** QUEUE CHANGED after {poll_count} polls ({elapsed:.1f}s) ***")
                print(f"  _version: {last_version} -> {curr_version}")
                print(f"  _size: {last_size} -> {curr_size}")

                # Read inventory scalars after the change
                after = read_inventory_scalars(pm, inv_addr)
                if after:
                    print(f"\nInventory deltas:")
                    any_delta = False
                    for k in before:
                        bv = before[k]
                        av = after[k]
                        if k == "vault_progress":
                            delta = av - bv
                            if abs(delta) > 0.0001:
                                print(f"  {k:20s}: {bv:.4f} -> {av:.4f}  (delta: {delta:+.4f}, raw_delta: {delta * 10:+.1f})")
                                any_delta = True
                        else:
                            delta = av - bv
                            if delta != 0:
                                print(f"  {k:20s}: {bv} -> {av}  (delta: {delta:+d})")
                                any_delta = True
                    if not any_delta:
                        print(f"  (no scalar changes detected)")

                # Read new report items
                if curr_size > 0:
                    # Re-read items array pointer (may have changed if List resized)
                    state2 = read_queue_state(pm, queue_list_ptr)
                    if state2:
                        curr_items_ptr = state2[0]

                    print(f"\nReading {curr_size} report item(s) from queue...")

                    for i in range(curr_size):
                        elem_ptr = read_i64(pm, curr_items_ptr + 32 + i * 8)
                        if elem_ptr is None or elem_ptr == 0:
                            continue

                        print(f"\n  -- Report Item [{i}] at {elem_ptr:#x} --")
                        item = read_report_item(pm, elem_ptr)
                        if item is None:
                            print(f"    (failed to read)")
                            continue

                        # Source type
                        source_type = read_context_source_type(pm, item["context_ptr"])
                        source_name = SOURCE_NAMES.get(source_type, f"unknown({source_type})")
                        print(f"    Source: {source_name} ({source_type})")
                        print(f"    XP gained: {item['xp_gained']}")

                        # Parent context string
                        if item["parent_ptr"] and (0x1_0000_0000 <= item["parent_ptr"] <= 0x7FFF_FFFF_FFFF):
                            parent_str = read_il2cpp_string(pm, item["parent_ptr"])
                            print(f"    Parent context: {parent_str}")
                        else:
                            print(f"    Parent context: null")

                        # ── THE MAIN EVENT: InventoryDelta dump ──
                        delta = dump_inventory_delta(pm, item["delta_ptr"])
                        if delta:
                            print(f"\n    InventoryDelta at {delta['address']:#x} (klass={delta['klass']:#x})")

                            # Reference fields
                            print(f"\n    Reference fields (+16 to +96):")
                            ref_names = [
                                "boosterDelta?", "boosterDelta2?", "cardsAdded?",
                                "decksAdded?", "vanityAdded?", "vanityRemoved?",
                                "artSkinsAdded?", "artSkinsRemoved?", "ref8?",
                                "ref9?", "ref10?"
                            ]
                            for j, (ptr, name) in enumerate(zip(delta["ref_ptrs"], ref_names)):
                                off = 16 + j * 8
                                if ptr == 0:
                                    print(f"      +{off:3d}: null  ({name})")
                                elif 0x1_0000_0000 <= ptr <= 0x7FFF_FFFF_FFFF:
                                    # Try to read what it points to
                                    target = read_memory(pm, ptr, 32)
                                    info = ""
                                    if target:
                                        t_klass = struct.unpack_from("<Q", target, 0)[0]
                                        # Check if it's a managed array (has max_length at +24)
                                        t_24 = struct.unpack_from("<Q", target, 24)[0]
                                        if t_24 < 1000:  # small = likely array length
                                            info = f" -> array? (klass={t_klass:#x}, max_length?={t_24})"
                                        else:
                                            info = f" -> (klass={t_klass:#x})"
                                    print(f"      +{off:3d}: {ptr:#x}  ({name}){info}")
                                else:
                                    print(f"      +{off:3d}: {ptr:#x}  (not a pointer)")

                            # Value fields — THE KEY OUTPUT
                            print(f"\n    Value fields (+104 onward):")
                            print(f"    {'Offset':>8s}  {'i32 val':>10s}  {'as hex':>12s}  {'f64 (if pair)':>16s}")
                            print(f"    {'-' * 50}")

                            vals = delta["value_i32s"]
                            f64s = delta["value_f64s"]

                            # Show all non-zero values, plus the first few zero values for context
                            for off in sorted(vals.keys()):
                                val = vals[off]
                                hex_val = f"0x{val & 0xFFFFFFFF:08x}"
                                f64_val = ""
                                if off in f64s and off % 8 == 0:
                                    fv = f64s[off]
                                    if fv != 0.0 and abs(fv) < 1e10:
                                        f64_val = f"{fv:.6f}"
                                marker = " <<<" if val != 0 else ""
                                if off <= 168 or val != 0:  # Show first 16 i32 slots always
                                    print(f"    +{off:3d}:    {val:>10d}  {hex_val:>12s}  {f64_val:>16s}{marker}")

                        # Aetherized cards
                        cards = read_aetherized_cards(pm, item["aetherized_ptr"])
                        if cards:
                            print(f"\n    Aetherized cards ({len(cards)}):")
                            for c in cards:
                                vault = f", vault={c['vaultProgress']:.2f}" if c["vaultProgress"] > 0 else ""
                                gold = f", gold={c['goldAwarded']}" if c["goldAwarded"] > 0 else ""
                                gems = f", gems={c['gemsAwarded']}" if c["gemsAwarded"] > 0 else ""
                                added = "NEW" if c["addedToInventory"] else "5th+"
                                print(f"      grpId={c['grpId']}, set={c['set']}, {added}{vault}{gold}{gems}")

                # Update state for next iteration
                before = after if after else before
                last_version = curr_version
                last_size = curr_size
                poll_count = 0
                print(f"\n\nContinuing to watch... (Ctrl+C to stop)")

    except KeyboardInterrupt:
        print("\nStopped.")

    pm.close_process()
    return 0


if __name__ == "__main__":
    sys.exit(main())
