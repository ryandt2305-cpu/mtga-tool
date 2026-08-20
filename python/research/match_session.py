"""Match session data vacuum — captures EVERYTHING during a game.

No re-plays needed. All raw data saved to JSON for post-analysis.
Sorts through later — does not pre-filter or assume what's relevant.

Monitors (50ms queue, 500ms scalars, 2s log tail, 30s debug server):
  - Report queue: 50ms — version/size check, full dump on ANY change
  - Inventory scalars: 500ms — gold, gems, WCs, vault, track
  - Rank state: 500ms — all 13 fields
  - Boosters: 500ms — full list via inv-64
  - Custom tokens: 500ms — full dict via inv-8
  - Mastery: 500ms — level + progress
  - Player.log tail: 2s — real-time JSON parsing + categorization
  - Debug server: 30s — periodic full dumps (catches anything we miss)

Before/after captures:
  - Debug server full dump
  - Player.log all new lines + parsed JSON objects
  - Collection summary (via debug server)
  - Cosmetics list sizes (if --cosmetics flag and address known)
  - Full state diff

Run: python -m python.research.match_session [--gold N] [--progress N] [--cosmetics]
"""
from __future__ import annotations

import argparse
import json
import struct
import sys
import time
from datetime import datetime
from pathlib import Path

from .probe import (
    enumerate_regions,
    find_inventory,
    open_mtga,
    read_i64,
    read_memory,
)
from .starthook import extract_starthook
from .delta_mapper import (
    find_inventory_client,
    read_queue_state,
)
from .vacuum_readers import (
    ts, ts_full,
    read_inventory_scalars,
    read_rank_state,
    read_boosters,
    read_custom_tokens,
    read_mastery_node,
    read_cosmetics_sizes,
    find_rank_address,
    find_mastery_address,
    fetch_debug_server,
    get_log_size,
    read_new_log_lines,
    parse_log_json_objects,
    capture_report_item,
    snapshot_collection_summary,
    save_session,
)


def print_report_item(item: dict, index: int) -> None:
    """Pretty-print a captured report item."""
    print(f"    [{index}] Source: {item['source_name']} ({item['source_type']})")
    print(f"        XP: {item['xp_gained']}, Parent: {item['parent_context']}")

    if item.get("delta_raw_values"):
        print(f"        Delta values (+104 to +159):")
        labels = {
            "104": "A:gems / B:vault_lo", "108": "A:gold / B:vault",
            "112": "A:vault_lo / B:vault", "116": "A:vault / B:vault_hi",
            "120": "A:vault / B:gems", "124": "A:vault_hi / B:gold",
            "128": "wcTrackPos", "132": "wcCommon", "136": "wcUncommon",
            "140": "wcRare", "144": "wcMythic",
            "148": "wcTrackUnc?", "152": "wcTrackRare?", "156": "wcTrackMyth?",
        }
        for off_str, val in sorted(item["delta_raw_values"].items(), key=lambda x: int(x[0])):
            label = labels.get(off_str, "")
            marker = " <<<" if val != 0 else ""
            print(f"          +{off_str}: {val:>10d}  {label}{marker}")

    if item.get("delta"):
        non_null = {k: v for k, v in item["delta"].items() if v is not None}
        if non_null:
            print(f"        Delta refs: {json.dumps(non_null, default=str)[:300]}")

    if item.get("aetherized_cards"):
        print(f"        Cards ({len(item['aetherized_cards'])}):")
        for c in item["aetherized_cards"][:10]:
            extras = []
            if c.get("vaultProgress", 0) > 0:
                extras.append(f"vault={c['vaultProgress']:.2f}")
            if c.get("goldAwarded", 0) > 0:
                extras.append(f"gold={c['goldAwarded']}")
            if c.get("gemsAwarded", 0) > 0:
                extras.append(f"gems={c['gemsAwarded']}")
            added = "NEW" if c.get("addedToInventory") else "5th+"
            extra_str = f" ({', '.join(extras)})" if extras else ""
            print(f"          grpId={c['grpId']}, set={c.get('set','?')}, {added}{extra_str}")


def main() -> int:
    parser = argparse.ArgumentParser(description="Comprehensive match session data vacuum")
    parser.add_argument("--gold", type=int, default=None,
                        help="Override gold value for inventory search")
    parser.add_argument("--progress", type=int, default=575,
                        help="Current mastery XP progress (default: 575)")
    parser.add_argument("--cosmetics", action="store_true",
                        help="Include cosmetics scan (adds ~100s to init)")
    args = parser.parse_args()

    sh = extract_starthook()
    overrides = {}
    if args.gold is not None:
        overrides["gold"] = args.gold

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

    session_start = datetime.now()
    session_id = session_start.strftime("%Y%m%d_%H%M%S")
    output_dir = Path("python/research/captures")
    output_dir.mkdir(exist_ok=True)
    output_file = output_dir / f"match_session_{session_id}.json"

    session_data = {
        "session_id": session_id,
        "start_time": ts_full(),
        "pid": None,
        "addresses": {},
        "before": {},
        "after": {},
        "changes": [],
        "queue_captures": [],
        "log_events": [],
        "debug_server_snapshots": [],
        "debug_server_before": None,
        "debug_server_after": None,
        "new_log_lines_count": 0,
        "new_log_json_objects": [],
    }

    print(f"Match Session Data Vacuum — {session_id}")
    print(f"Output: {output_file}")
    print(f"=" * 60)

    # ── Open process ──
    print(f"[{ts()}] Opening MTGA.exe...")
    pm = open_mtga()
    session_data["pid"] = pm.process_id
    print(f"[{ts()}] PID: {pm.process_id}")

    regions = enumerate_regions(pm)
    print(f"[{ts()}] {len(regions)} regions")

    # ── Find all targets ──
    print(f"[{ts()}] Finding inventory...")
    location = find_inventory(pm, regions, known)
    if location is None:
        print("ERROR: Inventory not found.")
        pm.close_process()
        return 1
    inv_addr = location.address
    session_data["addresses"]["inventory"] = inv_addr
    print(f"[{ts()}] Inventory at {inv_addr:#x}")

    print(f"[{ts()}] Finding InventoryClient + queue...")
    inv_obj_base = inv_addr - 80
    inv_client = find_inventory_client(pm, regions, inv_obj_base)
    queue_list_ptr = None
    if inv_client:
        session_data["addresses"]["inv_client"] = inv_client
        queue_list_ptr = read_i64(pm, inv_client + 112)
        if queue_list_ptr:
            session_data["addresses"]["queue_list"] = queue_list_ptr
            print(f"[{ts()}] Queue at {queue_list_ptr:#x}")
        else:
            print(f"[{ts()}] WARNING: Queue pointer null")
    else:
        print(f"[{ts()}] WARNING: InventoryClient not found")

    print(f"[{ts()}] Finding rank...")
    rank_addr = find_rank_address(pm, regions, sh)
    if rank_addr:
        session_data["addresses"]["rank"] = rank_addr
        print(f"[{ts()}] Rank at {rank_addr:#x}")
    else:
        print(f"[{ts()}] WARNING: Rank not found")

    print(f"[{ts()}] Finding mastery...")
    mastery_addr = find_mastery_address(pm, regions, args.progress)
    mastery_klass = None
    if mastery_addr:
        session_data["addresses"]["mastery"] = mastery_addr
        node = read_mastery_node(pm, mastery_addr)
        if node:
            mastery_klass = node["klass"]
            print(f"[{ts()}] Mastery at {mastery_addr:#x} (level={node['level']}, progress={node['progress']})")
    else:
        print(f"[{ts()}] WARNING: Mastery not found (progress={args.progress})")

    # Optional cosmetics scan
    cosm_addr = None
    if args.cosmetics:
        print(f"[{ts()}] Finding cosmetics (slow scan)...")
        from .full_discovery import discover_cosmetics
        cosm_result = discover_cosmetics(pm, regions, sh)
        if cosm_result:
            cosm_addr = cosm_result["address"]
            session_data["addresses"]["cosmetics"] = cosm_addr
            print(f"[{ts()}] Cosmetics at {cosm_addr:#x}")

    # ── Capture BEFORE state ──
    print(f"\n[{ts()}] Capturing pre-match state...")

    before = {}
    before["inventory"] = read_inventory_scalars(pm, inv_addr)
    before["boosters"] = read_boosters(pm, inv_addr)
    before["tokens"] = read_custom_tokens(pm, inv_addr)
    if rank_addr:
        before["rank"] = read_rank_state(pm, rank_addr)
    if mastery_addr:
        before["mastery"] = read_mastery_node(pm, mastery_addr)
    if queue_list_ptr:
        qs = read_queue_state(pm, queue_list_ptr)
        before["queue"] = {"items_ptr": qs[0], "size": qs[1], "version": qs[2]} if qs else None
    if cosm_addr:
        before["cosmetics"] = read_cosmetics_sizes(pm, cosm_addr)
    before["collection_summary"] = snapshot_collection_summary(pm, regions)

    session_data["before"] = before

    # Debug server full dump
    print(f"[{ts()}] Fetching debug server dump...")
    session_data["debug_server_before"] = fetch_debug_server("/api/dump")

    # Player.log offset
    log_start_offset = get_log_size()
    log_last_offset = log_start_offset
    session_data["log_start_offset"] = log_start_offset
    print(f"[{ts()}] Player.log offset: {log_start_offset}")

    # Print before state
    print(f"\n{'=' * 60}")
    print(f"PRE-MATCH STATE")
    print(f"{'=' * 60}")
    if before.get("inventory"):
        print(f"  Inventory: {before['inventory']}")
    if before.get("rank"):
        r = before["rank"]
        print(f"  Rank: Con(class={r['con_class']},lv={r['con_level']},step={r['con_step']},"
              f"W={r['con_wins']},D={r['con_draws']},L={r['con_losses']}) "
              f"Ltd(class={r['ltd_class']},lv={r['ltd_level']},step={r['ltd_step']},"
              f"W={r['ltd_wins']},L={r['ltd_losses']})")
    if before.get("boosters"):
        print(f"  Boosters: {before['boosters']}")
    if before.get("tokens"):
        print(f"  Tokens: {before['tokens']}")
    if before.get("mastery"):
        print(f"  Mastery: level={before['mastery']['level']}, progress={before['mastery']['progress']}")
    if before.get("queue"):
        print(f"  Queue: size={before['queue']['size']}, version={before['queue']['version']}")
    if before.get("cosmetics"):
        print(f"  Cosmetics: {before['cosmetics']}")
    if before.get("collection_summary"):
        print(f"  Collection: {before['collection_summary']}")

    save_session(output_file, session_data)

    # ── Watch loop ──
    print(f"\n{'=' * 60}")
    print(f"[{ts()}] WATCHING — play your game now")
    print(f"[{ts()}] Queue: 50ms | Scalars: 500ms | Log: 2s | Server: 30s")
    print(f"[{ts()}] Ctrl+C to stop and capture final state")
    print(f"{'=' * 60}\n")

    last_inv = before.get("inventory", {})
    last_rank = before.get("rank", {})
    last_boosters = before.get("boosters", [])
    last_tokens = before.get("tokens", {})
    last_mastery = before.get("mastery", {})
    last_q_version = before.get("queue", {}).get("version", -1)
    last_q_size = before.get("queue", {}).get("size", -1)

    poll_count = 0
    queue_poll_count = 0
    changes = []
    queue_captures = []
    log_events = []
    debug_snapshots = []
    seen_queue_items: set[int] = set()

    # Pre-populate seen items
    if queue_list_ptr and last_q_size > 0:
        qs = read_queue_state(pm, queue_list_ptr)
        if qs:
            for i in range(last_q_size):
                eptr = read_i64(pm, qs[0] + 32 + i * 8)
                if eptr:
                    seen_queue_items.add(eptr)

    try:
        while True:
            # ── Queue poll (50ms) ──
            time.sleep(0.05)
            queue_poll_count += 1

            if queue_list_ptr:
                qs = read_queue_state(pm, queue_list_ptr)
                if qs:
                    curr_items_ptr, curr_q_size, curr_q_version = qs
                    if curr_q_version != last_q_version:
                        print(f"\n*** [{ts()}] QUEUE CHANGED: v{last_q_version}->{curr_q_version}, "
                              f"size {last_q_size}->{curr_q_size} ***")
                        changes.append({
                            "ts": ts_full(), "type": "queue",
                            "old_version": last_q_version, "new_version": curr_q_version,
                            "old_size": last_q_size, "new_size": curr_q_size,
                        })
                        if curr_q_size > 0:
                            qs2 = read_queue_state(pm, queue_list_ptr)
                            if qs2:
                                curr_items_ptr = qs2[0]
                            for i in range(curr_q_size):
                                eptr = read_i64(pm, curr_items_ptr + 32 + i * 8)
                                if not eptr or not (0x1_0000_0000 <= eptr <= 0x7FFF_FFFF_FFFF):
                                    continue
                                is_new = eptr not in seen_queue_items
                                seen_queue_items.add(eptr)
                                captured = capture_report_item(pm, eptr)
                                if captured:
                                    captured["capture_time"] = ts_full()
                                    captured["is_new"] = is_new
                                    captured["queue_index"] = i
                                    captured["queue_version"] = curr_q_version
                                    queue_captures.append(captured)
                                    label = "NEW" if is_new else "existing"
                                    print(f"    [{i}] ({label}) {captured['source_name']} "
                                          f"(type={captured['source_type']}), XP={captured['xp_gained']}")
                                    print_report_item(captured, i)
                        last_q_version = curr_q_version
                        last_q_size = curr_q_size
                        session_data["changes"] = changes
                        session_data["queue_captures"] = queue_captures
                        save_session(output_file, session_data)

            # ── Everything else on different cadences ──
            if queue_poll_count % 10 != 0:
                continue

            poll_count += 1

            # Inventory scalars (every 500ms)
            curr_inv = read_inventory_scalars(pm, inv_addr)
            if curr_inv and last_inv:
                for k in curr_inv:
                    old = last_inv.get(k)
                    new = curr_inv[k]
                    if old is None:
                        continue
                    changed = (abs(new - old) > 0.0001) if k == "vault_raw" else (old != new)
                    if changed:
                        print(f"\n*** [{ts()}] INVENTORY: {k} {old} -> {new} ***")
                        changes.append({"ts": ts_full(), "type": "inventory",
                                        "field": k, "old": old, "new": new})
                last_inv = curr_inv

            # Rank (every 500ms)
            if rank_addr:
                curr_rank = read_rank_state(pm, rank_addr)
                if curr_rank and last_rank:
                    for k in curr_rank:
                        if curr_rank[k] != last_rank.get(k):
                            print(f"\n*** [{ts()}] RANK: {k} {last_rank.get(k)} -> {curr_rank[k]} ***")
                            changes.append({"ts": ts_full(), "type": "rank",
                                            "field": k, "old": last_rank.get(k), "new": curr_rank[k]})
                    last_rank = curr_rank

            # Boosters (every 500ms)
            curr_boosters = read_boosters(pm, inv_addr)
            if curr_boosters is not None and curr_boosters != last_boosters:
                print(f"\n*** [{ts()}] BOOSTERS CHANGED ***")
                changes.append({"ts": ts_full(), "type": "boosters",
                                "old": last_boosters, "new": curr_boosters})
                last_boosters = curr_boosters

            # Tokens (every 500ms)
            curr_tokens = read_custom_tokens(pm, inv_addr)
            if curr_tokens is not None and curr_tokens != last_tokens:
                print(f"\n*** [{ts()}] TOKENS CHANGED ***")
                changes.append({"ts": ts_full(), "type": "tokens",
                                "old": last_tokens, "new": curr_tokens})
                last_tokens = curr_tokens

            # Mastery (every 500ms)
            if mastery_addr and mastery_klass:
                curr_node = read_mastery_node(pm, mastery_addr)
                if curr_node and curr_node != last_mastery:
                    print(f"\n*** [{ts()}] MASTERY: level={curr_node['level']}, "
                          f"progress={curr_node['progress']} ***")
                    changes.append({"ts": ts_full(), "type": "mastery",
                                    "old": last_mastery, "new": curr_node})
                    last_mastery = curr_node
                elif curr_node is None:
                    for delta in range(-5, 6):
                        adj = mastery_addr + delta * 64
                        adj_node = read_mastery_node(pm, adj)
                        if (adj_node and adj_node.get("progress", 0) > 0
                                and adj_node["klass"] == mastery_klass):
                            print(f"\n*** [{ts()}] MASTERY (relocated): "
                                  f"level={adj_node['level']}, progress={adj_node['progress']} ***")
                            changes.append({"ts": ts_full(), "type": "mastery",
                                            "old": last_mastery, "new": adj_node})
                            last_mastery = adj_node
                            mastery_addr = adj
                            break

            # Player.log tail (every 2s = every 4th 500ms poll)
            if poll_count % 4 == 0:
                curr_log_size = get_log_size()
                if curr_log_size > log_last_offset:
                    new_lines = read_new_log_lines(log_last_offset)
                    log_last_offset = curr_log_size
                    new_json = parse_log_json_objects(new_lines)
                    for obj in new_json:
                        # Categorize by keys for quick identification
                        keys = set(obj.keys())
                        category = "unknown"
                        if "constructedMatchesWon" in keys:
                            category = "rank_progress"
                        elif "constructedRankInfo" in keys:
                            category = "rank_full"
                        elif "greToClientEvent" in keys or "greToClientMessages" in keys:
                            category = "gre_message"
                        elif "InventoryInfo" in keys or "Changes" in keys:
                            category = "inventory_change"
                        elif "QuestId" in keys or "QuestRewards" in keys:
                            category = "quest"
                        elif "CourseId" in keys or "InternalEventName" in keys:
                            category = "event_course"
                        elif "CurrentModule" in keys:
                            category = "module_change"
                        elif "Gold" in keys and "Gems" in keys:
                            category = "inventory_snapshot"
                        elif "matchId" in keys or "opponentScreenName" in keys:
                            category = "match_info"
                        elif "Formats" in keys:
                            category = "formats"
                        elif "MilestoneStates" in keys or "GraphState" in keys:
                            category = "mastery_graph"
                        elif "DailyWinTrackState" in keys or "WeeklyWinTrackState" in keys:
                            category = "periodic_rewards"

                        event = {"ts": ts_full(), "category": category, "data": obj}
                        log_events.append(event)

                        if category not in ("gre_message", "formats", "unknown"):
                            print(f"  [LOG] {category}: {str(obj)[:120]}")

            # Debug server periodic dump (every 30s = every 60th 500ms poll)
            if poll_count % 60 == 0:
                ds = fetch_debug_server("/api/dump")
                if ds:
                    debug_snapshots.append({"ts": ts_full(), "data": ds})
                    print(f"  [debug] snapshot #{len(debug_snapshots)}")

            # Heartbeat + periodic save (every 60s)
            if poll_count % 120 == 0:
                elapsed = queue_poll_count * 0.05
                print(f"[{ts()}] Watching... ({elapsed:.0f}s, {len(changes)} changes, "
                      f"{len(queue_captures)} captures, {len(log_events)} log events)")
                session_data["changes"] = changes
                session_data["queue_captures"] = queue_captures
                session_data["log_events"] = log_events
                session_data["debug_server_snapshots"] = debug_snapshots
                save_session(output_file, session_data)

    except KeyboardInterrupt:
        pass

    # ── Capture AFTER state ──
    print(f"\n\n{'=' * 60}")
    print(f"[{ts()}] Capturing post-match state...")
    print(f"{'=' * 60}")

    after = {}
    after["inventory"] = read_inventory_scalars(pm, inv_addr)
    after["boosters"] = read_boosters(pm, inv_addr)
    after["tokens"] = read_custom_tokens(pm, inv_addr)
    if rank_addr:
        after["rank"] = read_rank_state(pm, rank_addr)
    if mastery_addr:
        after["mastery"] = read_mastery_node(pm, mastery_addr)
        if mastery_klass:
            for delta in range(-10, 11):
                adj = mastery_addr + delta * 64
                adj_node = read_mastery_node(pm, adj)
                if (adj_node and adj_node.get("progress", 0) > 0
                        and adj_node["klass"] == mastery_klass):
                    after["mastery_active"] = adj_node
                    break
    if queue_list_ptr:
        qs = read_queue_state(pm, queue_list_ptr)
        after["queue"] = {"items_ptr": qs[0], "size": qs[1], "version": qs[2]} if qs else None
        if qs and qs[1] > 0:
            print(f"[{ts()}] Final queue dump ({qs[1]} items):")
            for i in range(qs[1]):
                eptr = read_i64(pm, qs[0] + 32 + i * 8)
                if eptr and (0x1_0000_0000 <= eptr <= 0x7FFF_FFFF_FFFF):
                    if eptr not in seen_queue_items:
                        captured = capture_report_item(pm, eptr)
                        if captured:
                            captured["capture_time"] = ts_full()
                            captured["is_new"] = True
                            captured["queue_index"] = i
                            captured["queue_version"] = qs[2]
                            queue_captures.append(captured)
                            print_report_item(captured, i)
    if cosm_addr:
        after["cosmetics"] = read_cosmetics_sizes(pm, cosm_addr)
    after["collection_summary"] = snapshot_collection_summary(pm, regions)

    session_data["after"] = after

    # Debug server final dump
    print(f"[{ts()}] Fetching final debug server dump...")
    session_data["debug_server_after"] = fetch_debug_server("/api/dump")

    # All remaining log lines
    print(f"[{ts()}] Reading final Player.log lines...")
    all_new_lines = read_new_log_lines(log_start_offset)
    session_data["new_log_lines_count"] = len(all_new_lines)
    session_data["new_log_json_objects"] = parse_log_json_objects(all_new_lines)
    print(f"[{ts()}] {len(all_new_lines)} total new lines, "
          f"{len(session_data['new_log_json_objects'])} JSON objects")

    # Finalize
    session_data["changes"] = changes
    session_data["queue_captures"] = queue_captures
    session_data["log_events"] = log_events
    session_data["debug_server_snapshots"] = debug_snapshots
    session_data["end_time"] = ts_full()

    # ── Print summary ──
    _print_summary(before, after, changes, queue_captures, log_events)

    save_session(output_file, session_data)
    print(f"\nAll data saved to: {output_file}")

    pm.close_process()
    return 0


def _print_summary(before: dict, after: dict, changes: list,
                   queue_captures: list, log_events: list) -> None:
    print(f"\n{'=' * 60}")
    print(f"SESSION SUMMARY")
    print(f"{'=' * 60}")

    # Inventory diff
    print(f"\nInventory (before -> after):")
    b_inv = before.get("inventory", {})
    a_inv = after.get("inventory", {})
    for k in a_inv:
        old = b_inv.get(k)
        new = a_inv[k]
        if k == "vault_raw" and old is not None:
            if abs(new - old) > 0.0001:
                print(f"  {k:20s}: {old:.4f} -> {new:.4f} (delta: {new - old:+.4f}) <<<")
            else:
                print(f"  {k:20s}: {old:.4f} (unchanged)")
        elif old != new:
            delta = new - old if isinstance(new, (int, float)) and isinstance(old, (int, float)) else "?"
            print(f"  {k:20s}: {old} -> {new} (delta: {delta:+}) <<<")
        else:
            print(f"  {k:20s}: {old} (unchanged)")

    # Rank diff
    if before.get("rank") and after.get("rank"):
        print(f"\nRank (before -> after):")
        for k in after["rank"]:
            old = before["rank"].get(k)
            new = after["rank"][k]
            if old != new:
                print(f"  {k:20s}: {old} -> {new} <<<")
            else:
                print(f"  {k:20s}: {old}")

    if before.get("boosters") != after.get("boosters"):
        print(f"\nBoosters: {before.get('boosters')} -> {after.get('boosters')}")
    if before.get("tokens") != after.get("tokens"):
        print(f"\nTokens: {before.get('tokens')} -> {after.get('tokens')}")
    if before.get("mastery") != after.get("mastery"):
        print(f"\nMastery: {before.get('mastery')} -> {after.get('mastery')}")
    if before.get("cosmetics") != after.get("cosmetics"):
        print(f"\nCosmetics: {before.get('cosmetics')} -> {after.get('cosmetics')}")
    if before.get("collection_summary") != after.get("collection_summary"):
        print(f"\nCollection: {before.get('collection_summary')} -> {after.get('collection_summary')}")

    # Queue captures
    print(f"\nQueue captures: {len(queue_captures)} report items")
    new_captures = [c for c in queue_captures if c.get("is_new")]
    print(f"  New items: {len(new_captures)}")
    for i, c in enumerate(new_captures):
        print(f"  [{i}] {c['source_name']} (type={c['source_type']}), XP={c['xp_gained']}")

    # Log events by category
    categories = {}
    for e in log_events:
        cat = e.get("category", "unknown")
        categories[cat] = categories.get(cat, 0) + 1
    if categories:
        print(f"\nLog events by category:")
        for cat, count in sorted(categories.items(), key=lambda x: -x[1]):
            print(f"  {cat:25s}: {count}")

    print(f"\nTotal changes: {len(changes)} | Log events: {len(log_events)}")


if __name__ == "__main__":
    sys.exit(main())
