"""One-command smoke test for the Unity extractor.

Just run:
  python tools/test_extract.py

It will auto-find MTGA, pick a small bundle, extract a few textures,
and tell you what happened. No arguments needed.
"""

from __future__ import annotations

import sys
from pathlib import Path

# Add parent so imports work when run directly
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from tools.unity_extract import bundle as bundle_mod
from tools.unity_extract import serialized as ser_mod
from tools.unity_extract.serialized import CLASS_TEXTURE2D
from tools.unity_extract import texture as tex_mod
from tools.unity_extract.decoders import decode_texture

STEAM = Path(r"C:\Program Files (x86)\Steam\steamapps\common\MTGA")
EPIC = Path(r"C:\Program Files\Epic Games\MagicTheGathering")


def find_mtga() -> Path | None:
    for p in [STEAM, EPIC]:
        if p.is_dir():
            return p
    return None


def main() -> None:
    print("=== Unity Extractor Smoke Test ===\n")

    # 1. Find MTGA
    install = find_mtga()
    if not install:
        print("MTGA not found at default paths.")
        print(f"  Checked: {STEAM}")
        print(f"  Checked: {EPIC}")
        print("\nTo test manually, pass a bundle path:")
        print("  python tools/test_extract.py path/to/file.mtga")
        return

    print(f"Found MTGA: {install}")

    bundle_dir = install / "MTGA_Data" / "Downloads" / "AssetBundle"
    if not bundle_dir.is_dir():
        bundle_dir = install / "MTGA_Data" / "Downloads"

    # 2. Pick a small bundle (sort by size, take smallest few)
    bundles = sorted(bundle_dir.glob("*.mtga"), key=lambda p: p.stat().st_size)
    bundles = [b for b in bundles if not b.name.startswith("Manifest_")]

    if not bundles:
        print(f"No .mtga bundles found in {bundle_dir}")
        return

    print(f"Found {len(bundles)} bundles total")
    print(f"Testing with smallest bundles...\n")

    out_dir = Path("tools/test_output")
    out_dir.mkdir(parents=True, exist_ok=True)

    textures_found = 0
    extracted = 0
    errors = 0

    # Try up to 10 small bundles until we extract something
    for bundle_path in bundles[:10]:
        size_kb = bundle_path.stat().st_size / 1024
        print(f"  {bundle_path.name} ({size_kb:.0f} KB)...")

        try:
            bnd = bundle_mod.parse_bundle(str(bundle_path))
        except Exception as e:
            print(f"    Skip (parse error: {e})")
            continue

        for entry in bnd.entries:
            if entry.name.endswith(".resS") or entry.name.endswith(".resource"):
                continue
            try:
                entry_data = bundle_mod.get_entry_data(bnd, entry)
                sf = ser_mod.parse_serialized_file(entry_data)
            except Exception:
                continue

            for obj in sf.objects_by_class(CLASS_TEXTURE2D):
                try:
                    obj_data = sf.get_object_data(obj)
                    tex = tex_mod.read_texture2d(obj_data, sf.big_endian)
                    textures_found += 1

                    # Resolve stream data if needed
                    if tex.has_stream_data and len(tex.image_data) == 0:
                        try:
                            tex.image_data = bundle_mod.resolve_stream_data(
                                bnd, tex.stream_path,
                                tex.stream_offset, tex.stream_size
                            )
                        except Exception as e:
                            print(f"    {tex.name}: stream error ({e})")
                            errors += 1
                            continue

                    img = decode_texture(tex)
                    safe_name = tex.name
                    for ch in r'<>:"/\|?*':
                        safe_name = safe_name.replace(ch, "_")
                    out_path = out_dir / f"{safe_name}.png"
                    img.save(out_path)
                    print(f"    Extracted: {tex.name} "
                          f"({tex.width}x{tex.height} {tex.format_name})")
                    extracted += 1
                except tex_mod.TextureDecodeError as e:
                    print(f"    {tex.name}: {e}")
                    errors += 1
                except Exception as e:
                    errors += 1

    print(f"\n=== Results ===")
    print(f"Textures found:     {textures_found}")
    print(f"Extracted as PNG:   {extracted}")
    print(f"Skipped/errors:     {errors}")
    if extracted > 0:
        print(f"Output directory:   {out_dir.resolve()}")
        print(f"\nOpen that folder to see the extracted PNGs.")
    elif textures_found > 0:
        print(f"\nTextures were found but none could be decoded.")
        print("This likely means they use BC7 format (not yet supported).")
    else:
        print(f"\nNo textures found in the smallest bundles.")
        print("Try: python tools/extract_mtga_icons.py --list-all")


if __name__ == "__main__":
    # Allow passing a specific bundle path as argument
    if len(sys.argv) > 1:
        path = Path(sys.argv[1])
        if not path.exists():
            print(f"File not found: {path}")
            sys.exit(1)
        print(f"Testing bundle: {path}\n")
        out_dir = Path("tools/test_output")
        out_dir.mkdir(parents=True, exist_ok=True)

        bnd = bundle_mod.parse_bundle(str(path))
        count = 0
        for entry in bnd.entries:
            if entry.name.endswith(".resS") or entry.name.endswith(".resource"):
                continue
            try:
                entry_data = bundle_mod.get_entry_data(bnd, entry)
                sf = ser_mod.parse_serialized_file(entry_data)
            except Exception:
                continue
            for obj in sf.objects_by_class(CLASS_TEXTURE2D):
                try:
                    obj_data = sf.get_object_data(obj)
                    tex = tex_mod.read_texture2d(obj_data, sf.big_endian)
                    if tex.has_stream_data and len(tex.image_data) == 0:
                        tex.image_data = bundle_mod.resolve_stream_data(
                            bnd, tex.stream_path,
                            tex.stream_offset, tex.stream_size
                        )
                    img = decode_texture(tex)
                    safe_name = tex.name
                    for ch in r'<>:"/\|?*':
                        safe_name = safe_name.replace(ch, "_")
                    out_path = out_dir / f"{safe_name}.png"
                    img.save(out_path)
                    print(f"  Extracted: {tex.name} "
                          f"({tex.width}x{tex.height} {tex.format_name})")
                    count += 1
                except Exception as e:
                    print(f"  {getattr(tex, 'name', '?')}: {e}")
        print(f"\n{count} texture(s) → {out_dir.resolve()}")
    else:
        main()
