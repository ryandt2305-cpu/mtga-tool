"""Extract economy/UI icons from MTGA game files.

Convenience script that finds the MTGA install, parses the asset manifest,
and extracts Texture2D assets matching economy icon patterns.

Usage:
  python tools/extract_mtga_icons.py --list-all           # discover texture names
  python tools/extract_mtga_icons.py -o public/icons/      # extract icons
  python tools/extract_mtga_icons.py --install-path "D:/Games/MTGA"  # custom path
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import sys
from pathlib import Path

from unity_extract import bundle as bundle_mod
from unity_extract import serialized as ser_mod
from unity_extract.serialized import CLASS_TEXTURE2D, CLASS_SPRITE, CLASS_SPRITE_ATLAS
from unity_extract import texture as tex_mod
from unity_extract import sprite as sprite_mod
from unity_extract import sprite_atlas as atlas_mod
from unity_extract.decoders import decode_texture

# MTGA install search paths (same as python/src/card_database.py)
STEAM_DEFAULT = Path(r"C:\Program Files (x86)\Steam\steamapps\common\MTGA")
EPIC_DEFAULT = Path(r"C:\Program Files\Epic Games\MagicTheGathering")
SEARCH_PATHS = [STEAM_DEFAULT, EPIC_DEFAULT]

# Texture name patterns likely to contain economy/UI icons
ICON_PATTERNS = [
    "*Gold*", "*Gem*", "*Wildcard*", "*Vault*", "*Booster*", "*Token*",
    "*Currency*", "*Resource*", "*Icon*", "*Reward*", "*Coin*",
    "*Common*", "*Uncommon*", "*Rare*", "*Mythic*",
]

# Manifest entry name patterns for UI/economy bundles (case-insensitive check)
# MTGA bundles use names like "Atlas_Home_xxx.mtga", "Shared_xxx.mtga" etc.
BUNDLE_HINT_PATTERNS = [
    "atlas_*", "*ui*", "*icon*", "*economy*", "*currency*", "*reward*",
    "*resource*", "*wildcard*", "*vault*", "*booster*", "*hud*",
    "shared_*", "*common*", "*store*", "*shop*", "*inventory*",
    "*navbar*", "*popout*", "*objective*", "*event*",
    "*misccardart*", "*cardart*",
]


def find_mtga_install(custom_path: str | None = None) -> Path:
    """Find MTGA install directory."""
    if custom_path:
        p = Path(custom_path)
        if p.is_dir():
            return p
        print(f"Error: custom path does not exist: {p}", file=sys.stderr)
        sys.exit(1)

    for p in SEARCH_PATHS:
        if p.is_dir():
            return p

    print("Error: MTGA install not found. Searched:", file=sys.stderr)
    for p in SEARCH_PATHS:
        print(f"  {p}", file=sys.stderr)
    print("Use --install-path to specify a custom location.", file=sys.stderr)
    sys.exit(1)


def parse_manifest(downloads_dir: Path) -> list[dict]:
    """Parse the MTGA asset manifest to get bundle names.

    The manifest is a JSON file named Manifest_*.mtga in the Downloads dir
    (parent of AssetBundle/).
    Structure: {"Assets": [{"Name": "...", "Length": N, "sha256": "..."}]}
    """
    manifests = list(downloads_dir.glob("Manifest_*.mtga"))
    if not manifests:
        print("Warning: no Manifest_*.mtga found, will scan all bundles",
              file=sys.stderr)
        return []

    # Use the main manifest (largest file, excludes Audio/Localization variants)
    manifests.sort(key=lambda p: p.stat().st_size, reverse=True)
    manifest_path = manifests[0]
    print(f"Using manifest: {manifest_path.name} "
          f"({manifest_path.stat().st_size:,} bytes)")

    try:
        with open(manifest_path, "r", encoding="utf-8") as f:
            data = json.load(f)
        return data.get("Assets", [])
    except (json.JSONDecodeError, KeyError) as e:
        print(f"Warning: failed to parse manifest: {e}", file=sys.stderr)
        return []


def filter_bundles(assets: list[dict], bundle_dir: Path) -> list[Path]:
    """Filter manifest entries to likely UI/economy bundles."""
    if not assets:
        # No manifest — fall back to scanning all .mtga files
        return sorted(bundle_dir.glob("*.mtga"))

    candidates = []
    for asset in assets:
        name = asset.get("Name", "")
        name_lower = name.lower()
        if any(fnmatch.fnmatch(name_lower, pat) for pat in BUNDLE_HINT_PATTERNS):
            full_path = bundle_dir / name
            if full_path.exists():
                candidates.append(full_path)

    if not candidates:
        print("No bundles matched hint patterns. Scanning all bundles...",
              file=sys.stderr)
        return sorted(bundle_dir.glob("*.mtga"))

    return candidates


def scan_bundle_textures(bundle_path: Path):
    """Extract all Texture2D metadata from a bundle.

    Yields (Texture2D, AssetBundle) tuples — bundle is needed for stream resolution.
    """
    try:
        bnd = bundle_mod.parse_bundle(str(bundle_path))
    except Exception:
        return

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
                yield tex, bnd
            except Exception:
                continue


def scan_bundle_sprites(bundle_path: Path):
    """Extract Sprite and atlas Texture2D data from a bundle.

    Yields (Sprite, texture_rect, packing_rotation, decoded_atlas_PIL_Image,
    AssetBundle) tuples. Resolves SpriteAtlas indirection for sprites with null
    texture PPtrs. Caches decoded atlas textures per bundle.
    """
    try:
        bnd = bundle_mod.parse_bundle(str(bundle_path))
    except Exception:
        return

    # Index Texture2D, Sprite, and SpriteAtlas objects
    tex_index: dict[int, tuple[tex_mod.Texture2D, ser_mod.SerializedFile]] = {}
    sprite_list: list[sprite_mod.Sprite] = []
    atlas_map: dict[tuple[bytes, int], atlas_mod.SpriteAtlasEntry] = {}

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
                tex_index[obj.path_id] = (tex, sf)
            except Exception:
                pass

        for obj in sf.objects_by_class(CLASS_SPRITE):
            try:
                obj_data = sf.get_object_data(obj)
                spr = sprite_mod.read_sprite(obj_data, sf.big_endian)
                sprite_list.append(spr)
            except Exception:
                pass

        for obj in sf.objects_by_class(CLASS_SPRITE_ATLAS):
            try:
                obj_data = sf.get_object_data(obj)
                entries = atlas_mod.read_sprite_atlas(obj_data, sf.big_endian)
                atlas_map.update(entries)
            except Exception:
                pass

    if not sprite_list:
        return

    decoded_cache: dict[int, object] = {}

    for spr in sprite_list:
        if spr.texture_file_id != 0:
            continue

        # Resolve texture reference — direct or SpriteAtlas indirection
        tex_path_id = spr.texture_path_id
        texture_rect = spr.texture_rect
        packing_rotation = spr.packing_rotation

        if tex_path_id == 0:
            atlas_entry = atlas_map.get(spr.render_data_key)
            if atlas_entry is None or atlas_entry.texture_file_id != 0:
                continue
            tex_path_id = atlas_entry.texture_path_id
            texture_rect = atlas_entry.texture_rect
            packing_rotation = atlas_entry.packing_rotation

        if tex_path_id not in tex_index:
            continue

        if tex_path_id not in decoded_cache:
            tex, _ = tex_index[tex_path_id]
            if tex.has_stream_data and len(tex.image_data) == 0:
                try:
                    tex.image_data = bundle_mod.resolve_stream_data(
                        bnd, tex.stream_path, tex.stream_offset, tex.stream_size
                    )
                except Exception:
                    continue
            try:
                decoded_cache[tex_path_id] = decode_texture(tex)
            except Exception:
                continue

        atlas_img = decoded_cache[tex_path_id]
        yield spr, texture_rect, packing_rotation, atlas_img, bnd


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Extract economy/UI icons from MTGA game files"
    )
    parser.add_argument("--install-path", "-i",
                        help="MTGA install directory (auto-detected if omitted)")
    parser.add_argument("--output", "-o", default="public/icons",
                        help="Output directory for PNGs (default: public/icons)")
    parser.add_argument("--list-all", action="store_true",
                        help="List all texture names without extracting")
    parser.add_argument("--pattern", "-p", action="append",
                        help="Additional glob patterns for texture names")
    parser.add_argument("--scan-all", action="store_true",
                        help="Scan all bundles, not just manifest-filtered ones")
    args = parser.parse_args()

    install = find_mtga_install(args.install_path)
    downloads_dir = install / "MTGA_Data" / "Downloads"
    bundle_dir = downloads_dir / "AssetBundle"

    if not bundle_dir.is_dir():
        if downloads_dir.is_dir():
            bundle_dir = downloads_dir
        else:
            print(f"Error: bundle directory not found: {bundle_dir}",
                  file=sys.stderr)
            sys.exit(1)

    print(f"MTGA install: {install}")
    print(f"Bundle dir:   {bundle_dir}")

    # Get bundle list
    if args.scan_all:
        bundles = sorted(bundle_dir.glob("*.mtga"))
    else:
        assets = parse_manifest(downloads_dir)
        bundles = filter_bundles(assets, bundle_dir)

    # Filter out the manifest itself
    bundles = [b for b in bundles if not b.name.startswith("Manifest_")]
    print(f"Scanning {len(bundles)} bundle(s)...\n")

    patterns = ICON_PATTERNS + (args.pattern or [])
    out_dir = Path(args.output)

    if not args.list_all:
        out_dir.mkdir(parents=True, exist_ok=True)

    extracted = 0
    listed = 0

    for bundle_path in bundles:
        for tex, bnd in scan_bundle_textures(bundle_path):
            if args.list_all:
                stream_flag = " [STREAM]" if tex.has_stream_data else ""
                print(f"  {bundle_path.name}: {tex.name} "
                      f"({tex.width}x{tex.height} {tex.format_name})"
                      f"{stream_flag}")
                listed += 1
                continue

            # Check if texture name matches any icon pattern
            if not any(fnmatch.fnmatch(tex.name, pat) for pat in patterns):
                continue

            # Resolve stream data if pixel data is external
            if tex.has_stream_data and len(tex.image_data) == 0:
                try:
                    tex.image_data = bundle_mod.resolve_stream_data(
                        bnd, tex.stream_path, tex.stream_offset, tex.stream_size
                    )
                except bundle_mod.BundleParseError as e:
                    print(f"  Cannot resolve stream for '{tex.name}': {e}",
                          file=sys.stderr)
                    continue

            try:
                img = decode_texture(tex)
                safe_name = tex.name
                for ch in r'<>:"/\|?*':
                    safe_name = safe_name.replace(ch, "_")
                out_path = out_dir / f"{safe_name}.png"
                img.save(out_path)
                print(f"  Extracted: {tex.name} ({tex.width}x{tex.height} "
                      f"{tex.format_name}) -> {out_path}")
                extracted += 1
            except tex_mod.TextureDecodeError as e:
                print(f"  Cannot decode '{tex.name}': {e}", file=sys.stderr)

    # Second pass: sprite slicing from atlas textures
    sliced = 0
    if not args.list_all:
        print("Scanning for atlas sprites...")
        for bundle_path in bundles:
            for spr, texture_rect, packing_rotation, atlas_img, bnd in scan_bundle_sprites(bundle_path):
                if not any(fnmatch.fnmatch(spr.name, pat) for pat in patterns):
                    continue

                atlas_w, atlas_h = atlas_img.size
                tx, ty, tw, th = texture_rect

                left = int(tx)
                bottom_y = int(ty)
                top_y = atlas_h - bottom_y - int(th)
                right = left + int(tw)
                bottom = top_y + int(th)

                left = max(0, left)
                top_y = max(0, top_y)
                right = min(atlas_w, right)
                bottom = min(atlas_h, bottom)

                if right <= left or bottom <= top_y:
                    continue

                sprite_img = atlas_img.crop((left, top_y, right, bottom))

                if packing_rotation != sprite_mod.PACK_ROTATION_NONE:
                    from PIL import Image
                    if packing_rotation == sprite_mod.PACK_ROTATION_FLIP_H:
                        sprite_img = sprite_img.transpose(Image.FLIP_LEFT_RIGHT)
                    elif packing_rotation == sprite_mod.PACK_ROTATION_FLIP_V:
                        sprite_img = sprite_img.transpose(Image.FLIP_TOP_BOTTOM)
                    elif packing_rotation == sprite_mod.PACK_ROTATION_ROTATE_180:
                        sprite_img = sprite_img.transpose(Image.ROTATE_180)
                    elif packing_rotation == sprite_mod.PACK_ROTATION_ROTATE_90:
                        sprite_img = sprite_img.transpose(Image.ROTATE_270)

                safe_name = spr.name
                for ch in r'<>:"/\|?*':
                    safe_name = safe_name.replace(ch, "_")
                out_path = out_dir / f"{safe_name}.png"
                sprite_img.save(out_path)
                print(f"  Sliced: {spr.name} ({int(tw)}x{int(th)}) -> {out_path}")
                sliced += 1

    if args.list_all:
        print(f"\n{listed} texture(s) found.")
    else:
        print(f"\n{extracted} icon(s) extracted, {sliced} sprite(s) sliced "
              f"to {out_dir}/")


if __name__ == "__main__":
    main()
