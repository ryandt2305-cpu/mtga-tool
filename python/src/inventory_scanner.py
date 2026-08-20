"""Research spike: find ClientPlayerInventory in MTGA.exe memory.

Scans for known field values and reports clusters where multiple
known values appear near each other — indicating the inventory object.

Known values from StartHook + user confirmation:
    gold = 1300 (may have changed)
    gems = 1400 (user confirmed current)
    wcCommon = 20 (may have changed from crafting)
    wcUncommon = 14 (may have changed)
    wcMythic = 1 (may have changed)
    wcTrackPosition = 22

.NET class layout (from data_audit.py section 4m):
    wcCommon, wcUncommon, wcRare, wcMythic,
    gold, gems, wcTrackPosition, vaultProgress(double), ...

Strategy: search for gems=1400, then check surrounding 200 bytes
for other known values.
"""

import struct
import ctypes
import ctypes.wintypes

# Windows API constants
PROCESS_VM_READ = 0x0010
PROCESS_QUERY_INFORMATION = 0x0400
MEM_COMMIT = 0x1000
PAGE_NOACCESS = 0x01
PAGE_GUARD = 0x100

kernel32 = ctypes.windll.kernel32


class MEMORY_BASIC_INFORMATION(ctypes.Structure):
    _fields_ = [
        ("BaseAddress", ctypes.c_void_p),
        ("AllocationBase", ctypes.c_void_p),
        ("AllocationProtect", ctypes.wintypes.DWORD),
        ("RegionSize", ctypes.c_size_t),
        ("State", ctypes.wintypes.DWORD),
        ("Protect", ctypes.wintypes.DWORD),
        ("Type", ctypes.wintypes.DWORD),
    ]


def find_pid(name: str) -> int | None:
    import subprocess
    result = subprocess.run(
        ["tasklist", "/FI", f"IMAGENAME eq {name}", "/FO", "CSV", "/NH"],
        capture_output=True, text=True,
    )
    for line in result.stdout.strip().split("\n"):
        if name.lower() in line.lower():
            parts = line.strip('"').split('","')
            return int(parts[1])
    return None


def scan_for_inventory():
    pid = find_pid("MTGA.exe")
    if not pid:
        print("MTGA.exe not found")
        return

    print(f"MTGA.exe PID: {pid}")

    handle = kernel32.OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, False, pid)
    if not handle:
        print("Failed to open process")
        return

    # Ask user for current values
    print("\nEnter CURRENT in-game values (press Enter to skip):")

    known = {}
    prompts = [
        ("gold", "Gold"),
        ("gems", "Gems"),
        ("wcCommon", "Common wildcards"),
        ("wcUncommon", "Uncommon wildcards"),
        ("wcRare", "Rare wildcards"),
        ("wcMythic", "Mythic wildcards"),
        ("wcTrackPosition", "WC track position"),
    ]

    for key, label in prompts:
        val = input(f"  {label}: ").strip()
        if val:
            known[key] = int(val)

    if len(known) < 2:
        print("Need at least 2 known values to search")
        kernel32.CloseHandle(handle)
        return

    print(f"\nSearching for {len(known)} known values: {known}")

    # Convert to bytes for searching
    search_values = {k: struct.pack("<i", v) for k, v in known.items()}

    # Pick the rarest value to search for (largest number = fewest false positives)
    primary_key = max(known, key=lambda k: known[k])
    primary_bytes = search_values[primary_key]
    primary_val = known[primary_key]
    print(f"Primary search key: {primary_key}={primary_val}")

    # Scan memory
    address = 0
    max_address = (1 << 47) - 1
    mbi = MEMORY_BASIC_INFORMATION()
    mbi_size = ctypes.sizeof(mbi)

    hits = []
    regions_scanned = 0

    while address < max_address:
        result = kernel32.VirtualQueryEx(handle, ctypes.c_void_p(address), ctypes.byref(mbi), mbi_size)
        if result == 0:
            break

        region_size = mbi.RegionSize
        if region_size == 0:
            break

        if (mbi.State == MEM_COMMIT and
            mbi.Protect not in (PAGE_NOACCESS, PAGE_GUARD, 0) and
            not (mbi.Protect & PAGE_GUARD)):

            # Read region
            buf = ctypes.create_string_buffer(region_size)
            bytes_read = ctypes.c_size_t(0)
            if kernel32.ReadProcessMemory(handle, ctypes.c_void_p(address), buf, region_size, ctypes.byref(bytes_read)):
                data = buf.raw[:bytes_read.value]

                # Search for primary value
                pos = 0
                while True:
                    idx = data.find(primary_bytes, pos)
                    if idx == -1:
                        break

                    # Check surrounding 256 bytes for other known values
                    window_start = max(0, idx - 128)
                    window_end = min(len(data), idx + 128)
                    window = data[window_start:window_end]
                    window_base = address + window_start

                    matches = {}
                    for key, val_bytes in search_values.items():
                        offset = 0
                        while True:
                            found = window.find(val_bytes, offset)
                            if found == -1:
                                break
                            abs_addr = window_base + found
                            matches.setdefault(key, []).append(abs_addr)
                            offset = found + 1

                    if len(matches) >= 3:  # At least 3 different known values nearby
                        hits.append({
                            "address": address + idx,
                            "matches": matches,
                            "match_count": len(matches),
                        })

                    pos = idx + 4

            regions_scanned += 1

        address += region_size

    kernel32.CloseHandle(handle)

    print(f"\nScanned {regions_scanned} regions")
    print(f"Found {len(hits)} candidate clusters\n")

    if not hits:
        print("No clusters found. Values may have changed since input.")
        return

    # Sort by match count descending
    hits.sort(key=lambda h: h["match_count"], reverse=True)

    # Show top candidates
    for i, hit in enumerate(hits[:10]):
        print(f"--- Candidate {i+1}: {hit['match_count']} matches near {hit['address']:#x} ---")
        for key, addrs in sorted(hit["matches"].items()):
            for addr in addrs:
                print(f"  {key:20s} = {known[key]:6d}  at {addr:#x}  (offset {addr - hit['address']:+d})")

        # Dump the raw bytes around the hit for analysis
        print(f"\n  Dumping 256 bytes around {hit['address']:#x}:")
        dump_addr = hit["address"] - 64

        h2 = kernel32.OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, False, pid)
        buf = ctypes.create_string_buffer(256)
        bytes_read = ctypes.c_size_t(0)
        if kernel32.ReadProcessMemory(h2, ctypes.c_void_p(dump_addr), buf, 256, ctypes.byref(bytes_read)):
            data = buf.raw[:bytes_read.value]
            # Print as i32 values with offsets
            print(f"  {'Offset':>8s}  {'Hex':>10s}  {'i32':>10s}  {'u32':>10s}")
            for j in range(0, len(data) - 3, 4):
                val_i32 = struct.unpack_from("<i", data, j)[0]
                val_u32 = struct.unpack_from("<I", data, j)[0]
                addr = dump_addr + j
                marker = ""
                for key, val in known.items():
                    if val_i32 == val:
                        marker = f"  <-- {key}"
                        break
                print(f"  {addr:#010x}  {val_u32:#010x}  {val_i32:10d}  {val_u32:10d}{marker}")
        kernel32.CloseHandle(h2)
        print()


if __name__ == "__main__":
    scan_for_inventory()
