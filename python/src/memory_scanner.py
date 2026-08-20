"""Scan MTGA process memory for the card collection data structure.

MTGA uses C#/Mono. The collection is stored as a Dictionary<int, int>.
.NET Dictionary entries are 16-byte structs:

    struct Entry {
        int hashCode;   // key & 0x7FFFFFFF
        int next;       // chain index, or -1
        int key;        // the card GrpId
        int value;      // the quantity owned
    }

We scan in 16-byte strides and validate the hashCode matches the key.
"""
from __future__ import annotations

import struct
from dataclasses import dataclass, field

import pymem
import pymem.memory
from pymem.ressources.structure import MEMORY_BASIC_INFORMATION

# Memory region filtering
MEM_COMMIT = 0x1000
PAGE_NOACCESS = 0x01
PAGE_GUARD = 0x100

# Scan parameters
CHUNK_SIZE = 4 * 1024 * 1024  # 4 MB read chunks
ENTRY_SIZE = 16  # sizeof(Dictionary.Entry) = 4 int32s
MIN_VALID_ENTRIES = 100  # Minimum valid entries for a candidate block
MAX_QUANTITY = 250  # Max reasonable card quantity
MAX_GAP = 8  # Max consecutive invalid entries before breaking a run
MIN_DENSITY = 0.4  # Minimum ratio of valid/total entries in a candidate

# Scoring weights
WEIGHT_COUNT = 1.0
WEIGHT_DENSITY = 500.0
WEIGHT_ANCHOR = 200.0


@dataclass
class CandidateBlock:
    """A contiguous memory region containing dense collection-like data."""

    address: int
    entries: list[tuple[int, int]] = field(default_factory=list)  # (card_id, quantity)
    total_entries: int = 0
    anchor_hits: int = 0
    anchor_total: int = 0

    @property
    def density(self) -> float:
        if self.total_entries == 0:
            return 0.0
        return len(self.entries) / self.total_entries

    @property
    def score(self) -> float:
        anchor_ratio = (
            self.anchor_hits / self.anchor_total if self.anchor_total else 0.0
        )
        return (
            WEIGHT_COUNT * len(self.entries)
            + WEIGHT_DENSITY * self.density
            + WEIGHT_ANCHOR * anchor_ratio
        )


def _is_readable(mbi: MEMORY_BASIC_INFORMATION) -> bool:
    """Check if a memory region is committed and readable."""
    if mbi.State != MEM_COMMIT:
        return False
    if mbi.Protect & PAGE_NOACCESS or mbi.Protect & PAGE_GUARD:
        return False
    if mbi.Protect == 0:
        return False
    return True


def _enumerate_regions(
    pm: pymem.Pymem,
) -> list[tuple[int, int]]:
    """Enumerate all readable committed memory regions."""
    regions = []
    address = 0
    max_address = (1 << 47) - 1  # User-space limit on 64-bit Windows

    while address < max_address:
        try:
            mbi = pymem.memory.virtual_query(pm.process_handle, address)
        except Exception:
            break

        if mbi.RegionSize == 0:
            break

        if _is_readable(mbi) and mbi.RegionSize > ENTRY_SIZE * MIN_VALID_ENTRIES:
            regions.append((mbi.BaseAddress, mbi.RegionSize))

        address = mbi.BaseAddress + mbi.RegionSize

    return regions


def _is_valid_entry(
    hash_code: int,
    next_idx: int,
    key: int,
    value: int,
    valid_ids: set[int],
) -> bool:
    """Check if a 16-byte block looks like a Dictionary<int,int> entry.

    For int keys, .NET computes hashCode = key & 0x7FFFFFFF.
    next is either -1 (end of chain) or a small non-negative index.
    """
    if key not in valid_ids:
        return False
    if value < 1 or value > MAX_QUANTITY:
        return False
    # hashCode must equal key & 0x7FFFFFFF
    expected_hash = key & 0x7FFFFFFF
    if hash_code != expected_hash:
        return False
    # next should be -1 or a reasonable array index (< 100k for a collection)
    if next_idx != -1 and (next_idx < 0 or next_idx > 100_000):
        return False
    return True


def _scan_region(
    pm: pymem.Pymem,
    base: int,
    size: int,
    valid_ids: set[int],
    anchor_ids: set[int],
) -> list[CandidateBlock]:
    """Scan a memory region for runs of valid Dictionary<int,int> entries."""
    candidates: list[CandidateBlock] = []

    offset = 0
    while offset < size:
        read_size = min(CHUNK_SIZE, size - offset)
        try:
            data = pm.read_bytes(base + offset, read_size)
        except Exception:
            offset += read_size
            continue

        num_entries = len(data) // ENTRY_SIZE
        current_run: list[tuple[int, int]] = []
        run_start = 0
        gap = 0
        total_in_run = 0

        for i in range(num_entries):
            pos = i * ENTRY_SIZE
            hash_code, next_idx, key, value = struct.unpack_from("<iiii", data, pos)

            if _is_valid_entry(hash_code, next_idx, key, value, valid_ids):
                if gap > 0:
                    total_in_run += gap
                    gap = 0
                current_run.append((key, value))
                total_in_run += 1
            else:
                gap += 1
                if gap > MAX_GAP:
                    # Run ended — evaluate
                    if len(current_run) >= MIN_VALID_ENTRIES:
                        block = CandidateBlock(
                            address=base + offset + run_start * ENTRY_SIZE,
                            entries=current_run,
                            total_entries=total_in_run,
                            anchor_total=len(anchor_ids),
                        )
                        if block.density >= MIN_DENSITY:
                            found_ids = {p[0] for p in current_run}
                            block.anchor_hits = len(found_ids & anchor_ids)
                            candidates.append(block)
                    current_run = []
                    total_in_run = 0
                    gap = 0
                    run_start = i + 1

        # Handle run that extends to end of chunk
        if len(current_run) >= MIN_VALID_ENTRIES:
            block = CandidateBlock(
                address=base + offset + run_start * ENTRY_SIZE,
                entries=current_run,
                total_entries=total_in_run,
                anchor_total=len(anchor_ids),
            )
            if block.density >= MIN_DENSITY:
                found_ids = {p[0] for p in current_run}
                block.anchor_hits = len(found_ids & anchor_ids)
                candidates.append(block)

        offset += read_size

    return candidates


@dataclass
class ScanResult:
    """Result of a memory scan with verification data."""

    collection: dict[int, int]  # card_id → quantity (from best block)
    candidate_count: int
    cross_validation: CrossValidation | None = None


@dataclass
class CrossValidation:
    """Cross-validation between the top two memory copies."""

    block_a_unique: int
    block_b_unique: int
    shared_ids: int
    quantity_matches: int  # shared cards where both blocks agree on quantity
    quantity_mismatches: int  # shared cards where quantities differ
    only_in_a: int  # cards only in the best block
    only_in_b: int  # cards only in the runner-up


def _build_collection(
    block: CandidateBlock,
    non_collectible_ids: set[int],
) -> dict[int, int]:
    """Build a deduplicated collection dict from a candidate block."""
    collection: dict[int, int] = {}
    for card_id, quantity in block.entries:
        if card_id not in non_collectible_ids:
            collection[card_id] = max(collection.get(card_id, 0), quantity)
    return collection


def _cross_validate(
    col_a: dict[int, int],
    col_b: dict[int, int],
) -> CrossValidation:
    """Compare two collection reads for consistency."""
    ids_a = set(col_a)
    ids_b = set(col_b)
    shared = ids_a & ids_b

    qty_match = sum(1 for cid in shared if col_a[cid] == col_b[cid])
    qty_mismatch = len(shared) - qty_match

    return CrossValidation(
        block_a_unique=len(ids_a),
        block_b_unique=len(ids_b),
        shared_ids=len(shared),
        quantity_matches=qty_match,
        quantity_mismatches=qty_mismatch,
        only_in_a=len(ids_a - ids_b),
        only_in_b=len(ids_b - ids_a),
    )


def scan_collection(
    valid_ids: set[int],
    anchor_ids: set[int] | None = None,
    non_collectible_ids: set[int] | None = None,
    progress_callback=None,
) -> ScanResult:
    """Scan MTGA.exe process memory for the card collection.

    Args:
        valid_ids: Set of all valid collectible card GrpIds from the database.
        anchor_ids: Card IDs from user decks (soft validation, optional).
        non_collectible_ids: Card IDs to exclude from results.
        progress_callback: Optional callable(current_region, total_regions).

    Returns:
        ScanResult with collection data and cross-validation info.

    Raises:
        pymem.exception.ProcessNotFound: If MTGA.exe is not running.
        RuntimeError: If no collection data is found in memory.
    """
    if anchor_ids is None:
        anchor_ids = set()
    if non_collectible_ids is None:
        non_collectible_ids = set()

    # Exclude non-collectible IDs from the valid set used for scanning
    scan_ids = valid_ids - non_collectible_ids

    pm = pymem.Pymem("MTGA.exe")
    try:
        regions = _enumerate_regions(pm)
        print(f"  Found {len(regions)} readable memory regions")

        all_candidates: list[CandidateBlock] = []
        for idx, (base, size) in enumerate(regions):
            if progress_callback:
                progress_callback(idx + 1, len(regions))

            candidates = _scan_region(pm, base, size, scan_ids, anchor_ids)
            all_candidates.extend(candidates)

    finally:
        pm.close_process()

    if not all_candidates:
        raise RuntimeError(
            "No collection data found in MTGA memory. "
            "Make sure your collection is loaded in the game client "
            "(open the Decks or Collection tab)."
        )

    # Select the best candidate
    all_candidates.sort(key=lambda c: c.score, reverse=True)
    best = all_candidates[0]

    print(f"  Found {len(all_candidates)} candidate blocks")
    print(f"  Best block: {len(best.entries)} cards, "
          f"density={best.density:.1%}, "
          f"anchor coverage={best.anchor_hits}/{best.anchor_total}, "
          f"score={best.score:.0f}")

    if len(all_candidates) > 1:
        second = all_candidates[1]
        print(f"  Runner-up:  {len(second.entries)} cards, "
              f"density={second.density:.1%}, "
              f"score={second.score:.0f}")

    collection = _build_collection(best, non_collectible_ids)

    # Cross-validate with runner-up if available
    cross = None
    if len(all_candidates) >= 2:
        col_b = _build_collection(all_candidates[1], non_collectible_ids)
        cross = _cross_validate(collection, col_b)

    return ScanResult(
        collection=collection,
        candidate_count=len(all_candidates),
        cross_validation=cross,
    )
