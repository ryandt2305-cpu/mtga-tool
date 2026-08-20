"""Process access and inventory location for MTGA memory research.

Opens MTGA.exe with read-only access, enumerates memory regions, and locates
the ClientPlayerInventory struct using known StartHook values as anchors.
Also provides raw memory reading for dump and analysis commands.

Uses pymem for process access (same dependency as the collection scanner).
"""
from __future__ import annotations

import struct
from dataclasses import dataclass

import pymem
import pymem.memory
from pymem.ressources.structure import MEMORY_BASIC_INFORMATION

# Memory region filtering
MEM_COMMIT = 0x1000
MEM_PRIVATE = 0x20000
PAGE_NOACCESS = 0x01
PAGE_GUARD = 0x100

# Inventory struct layout (from Rust inventory.rs — verified)
INVENTORY_STRUCT_SIZE = 40
FIELD_OFFSETS = {
    "wc_common": 0,
    "wc_uncommon": 4,
    "wc_rare": 8,
    "wc_mythic": 12,
    "gold": 16,
    "gems": 20,
    "wc_track_position": 24,
    # 28 = padding for f64 alignment
    "vault_progress": 32,  # f64, 8 bytes
}

CHUNK_SIZE = 4 * 1024 * 1024  # 4 MB scan chunks


@dataclass
class MemRegion:
    """A readable committed memory region."""
    base: int
    size: int


@dataclass
class InventoryLocation:
    """Result of locating the inventory struct."""
    address: int
    score: int
    candidates_found: int
    scalars: dict[str, int | float]


def open_mtga() -> pymem.Pymem:
    """Open MTGA.exe process. Raises pymem.exception.ProcessNotFound if not running."""
    return pymem.Pymem("MTGA.exe")


def enumerate_regions(pm: pymem.Pymem) -> list[MemRegion]:
    """Enumerate all readable committed private memory regions."""
    regions: list[MemRegion] = []
    address = 0
    max_address = (1 << 47) - 1

    while address < max_address:
        try:
            mbi = pymem.memory.virtual_query(pm.process_handle, address)
        except Exception:
            break

        if mbi.RegionSize == 0:
            break

        if _is_scannable(mbi):
            regions.append(MemRegion(base=mbi.BaseAddress, size=mbi.RegionSize))

        address = mbi.BaseAddress + mbi.RegionSize

    return regions


def find_inventory(
    pm: pymem.Pymem,
    regions: list[MemRegion],
    known_values: dict[str, int],
) -> InventoryLocation | None:
    """Locate the ClientPlayerInventory struct using anchor-based search.

    known_values: dict mapping field names to known i32 values.
    Keys must be in FIELD_OFFSETS. At least 3 non-zero values required.
    """
    # Build known fields: (name, offset, value)
    known_fields: list[tuple[str, int, int]] = []
    for name, value in known_values.items():
        if name in FIELD_OFFSETS and value != 0:
            known_fields.append((name, FIELD_OFFSETS[name], value))

    if len(known_fields) < 3:
        raise ValueError(
            f"Need at least 3 non-zero known values, got {len(known_fields)}: "
            f"{[f[0] for f in known_fields]}"
        )

    # Primary = largest absolute value (fewest false positives)
    primary_name, primary_offset, primary_value = max(
        known_fields, key=lambda f: abs(f[2])
    )
    primary_bytes = struct.pack("<i", primary_value)

    best_addr: int | None = None
    best_score = 0
    best_scalars: dict[str, int | float] | None = None
    total_candidates = 0

    for region in regions:
        if region.size < INVENTORY_STRUCT_SIZE:
            continue

        offset = 0
        while offset < region.size:
            read_size = min(CHUNK_SIZE, region.size - offset)
            try:
                data = pm.read_bytes(region.base + offset, read_size)
            except Exception:
                offset += read_size
                continue

            # Search for primary value occurrences
            pos = 0
            while True:
                idx = data.find(primary_bytes, pos)
                if idx == -1:
                    break

                hit_addr = region.base + offset + idx
                if hit_addr < primary_offset:
                    pos = idx + 1
                    continue

                struct_base = hit_addr - primary_offset

                # Read the full 40-byte struct
                try:
                    struct_data = pm.read_bytes(struct_base, INVENTORY_STRUCT_SIZE)
                except Exception:
                    pos = idx + 1
                    continue

                # Score: count matching fields
                score = 0
                for _, foffset, fvalue in known_fields:
                    actual = struct.unpack_from("<i", struct_data, foffset)[0]
                    if actual == fvalue:
                        score += 1

                if score >= 3:
                    total_candidates += 1
                    scalars = _parse_inventory_struct(struct_data)
                    if scalars and _validate_plausibility(scalars) and score > best_score:
                        best_score = score
                        best_addr = struct_base
                        best_scalars = scalars

                pos = idx + 1

            offset += read_size

    if best_addr is not None and best_scalars is not None:
        return InventoryLocation(
            address=best_addr,
            score=best_score,
            candidates_found=total_candidates,
            scalars=best_scalars,
        )

    return None


def read_memory(pm: pymem.Pymem, address: int, size: int) -> bytes | None:
    """Read raw bytes from process memory. Returns None on failure."""
    try:
        return pm.read_bytes(address, size)
    except Exception:
        return None


def read_i32(pm: pymem.Pymem, address: int) -> int | None:
    """Read a single i32 from process memory."""
    data = read_memory(pm, address, 4)
    if data and len(data) == 4:
        return struct.unpack("<i", data)[0]
    return None


def read_i64(pm: pymem.Pymem, address: int) -> int | None:
    """Read a single i64 from process memory."""
    data = read_memory(pm, address, 8)
    if data and len(data) == 8:
        return struct.unpack("<q", data)[0]
    return None


def read_f64(pm: pymem.Pymem, address: int) -> float | None:
    """Read a single f64 from process memory."""
    data = read_memory(pm, address, 8)
    if data and len(data) == 8:
        return struct.unpack("<d", data)[0]
    return None


def _is_scannable(mbi: MEMORY_BASIC_INFORMATION) -> bool:
    """Check if a region is committed, private, and readable."""
    if mbi.State != MEM_COMMIT:
        return False
    # MEM_PRIVATE = 0x20000 — .NET GC heap is always private
    if mbi.Type != MEM_PRIVATE:
        return False
    if mbi.Protect & PAGE_NOACCESS or mbi.Protect & PAGE_GUARD:
        return False
    if mbi.Protect == 0:
        return False
    if mbi.RegionSize < INVENTORY_STRUCT_SIZE:
        return False
    return True


def _parse_inventory_struct(data: bytes) -> dict[str, int | float] | None:
    """Parse a 40-byte buffer into inventory scalars."""
    if len(data) < INVENTORY_STRUCT_SIZE:
        return None

    def read_i32_at(off: int) -> int:
        return struct.unpack_from("<i", data, off)[0]

    vault_bytes = data[32:40]
    vault = struct.unpack("<d", vault_bytes)[0]

    return {
        "wc_common": read_i32_at(0),
        "wc_uncommon": read_i32_at(4),
        "wc_rare": read_i32_at(8),
        "wc_mythic": read_i32_at(12),
        "gold": read_i32_at(16),
        "gems": read_i32_at(20),
        "wc_track_position": read_i32_at(24),
        "vault_progress": vault,
    }


def _validate_plausibility(s: dict[str, int | float]) -> bool:
    """Check if parsed values are within plausible game ranges."""
    return (
        s["wc_common"] >= 0
        and s["wc_uncommon"] >= 0
        and s["wc_rare"] >= 0
        and s["wc_mythic"] >= 0
        and s["gold"] >= 0
        and s["gems"] >= 0
        and s["wc_track_position"] >= 0
        and 0.0 <= s["vault_progress"] <= 200.0
    )
