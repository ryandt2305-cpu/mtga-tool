"""CLI interface for Unity asset bundle extraction.

Subcommands:
  list          — List Texture2D assets in a bundle
  extract       — Extract textures as PNG files
  scan          — Scan directory of bundles for matching textures
  list-sprites  — List Sprite assets in a bundle
  slice         — Slice sprites from atlas textures as individual PNGs
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import sys
from pathlib import Path

from . import bundle as bundle_mod
from . import serialized as ser_mod
from .serialized import CLASS_TEXTURE2D, CLASS_SPRITE, CLASS_SPRITE_ATLAS
from . import texture as tex_mod
from . import sprite as sprite_mod
from . import sprite_atlas as atlas_mod
from .decoders import decode_texture


def _safe_filename(name: str) -> str:
    """Sanitize a texture name for use as a filename."""
    # Replace characters invalid on Windows
    for ch in r'<>:"/\|?*':
        name = name.replace(ch, "_")
    return name


def _serialized_entries(bnd: bundle_mod.AssetBundle):
    """Yield only entries that are serialized files (skip .resS resource data)."""
    for entry in bnd.entries:
        if entry.name.endswith(".resS") or entry.name.endswith(".resource"):
            continue
        yield entry


def _list_textures(args: argparse.Namespace) -> None:
    """List all Texture2D assets in a bundle file."""
    path = args.bundle
    try:
        bnd = bundle_mod.parse_bundle(path)
    except bundle_mod.BundleParseError as e:
        print(f"Error parsing bundle: {e}", file=sys.stderr)
        sys.exit(1)

    results = []
    for entry in _serialized_entries(bnd):
        try:
            entry_data = bundle_mod.get_entry_data(bnd, entry)
            sf = ser_mod.parse_serialized_file(entry_data)
        except (ser_mod.SerializedFileError, Exception) as e:
            print(f"  Warning: failed to parse entry '{entry.name}': {e}",
                  file=sys.stderr)
            continue

        for obj in sf.objects_by_class(CLASS_TEXTURE2D):
            try:
                obj_data = sf.get_object_data(obj)
                tex = tex_mod.read_texture2d(obj_data, sf.big_endian)
                info = {
                    "name": tex.name,
                    "width": tex.width,
                    "height": tex.height,
                    "format": tex.format_name,
                    "data_size": len(tex.image_data),
                    "stream": tex.has_stream_data,
                    "entry": entry.name,
                }
                results.append(info)
            except (tex_mod.TextureDecodeError, Exception) as e:
                print(f"  Warning: failed to read texture in '{entry.name}': {e}",
                      file=sys.stderr)

    if args.json:
        print(json.dumps(results, indent=2))
    else:
        if not results:
            print("No Texture2D objects found.")
            return
        for r in results:
            stream_flag = " [STREAM]" if r["stream"] else ""
            print(f"  {r['name']:40s}  {r['width']:5d}x{r['height']:<5d}  "
                  f"{r['format']:10s}  {r['data_size']:>8d}B{stream_flag}")


def _extract_textures(args: argparse.Namespace) -> None:
    """Extract matching textures as PNG files."""
    path = args.bundle
    pattern = args.pattern or "*"
    out_dir = Path(args.output) if args.output else Path(".")
    out_dir.mkdir(parents=True, exist_ok=True)

    try:
        bnd = bundle_mod.parse_bundle(path)
    except bundle_mod.BundleParseError as e:
        print(f"Error parsing bundle: {e}", file=sys.stderr)
        sys.exit(1)

    extracted = 0
    for entry in _serialized_entries(bnd):
        try:
            entry_data = bundle_mod.get_entry_data(bnd, entry)
            sf = ser_mod.parse_serialized_file(entry_data)
        except Exception as e:
            print(f"  Warning: skipping entry '{entry.name}': {e}",
                  file=sys.stderr)
            continue

        for obj in sf.objects_by_class(CLASS_TEXTURE2D):
            try:
                obj_data = sf.get_object_data(obj)
                tex = tex_mod.read_texture2d(obj_data, sf.big_endian)
            except Exception as e:
                print(f"  Warning: failed to read texture: {e}",
                      file=sys.stderr)
                continue

            if not fnmatch.fnmatch(tex.name, pattern):
                continue

            # Resolve stream data if pixel data is external
            if tex.has_stream_data and len(tex.image_data) == 0:
                try:
                    tex.image_data = bundle_mod.resolve_stream_data(
                        bnd, tex.stream_path, tex.stream_offset, tex.stream_size
                    )
                except bundle_mod.BundleParseError as e:
                    print(f"  Warning: cannot resolve stream for "
                          f"'{tex.name}': {e}", file=sys.stderr)
                    continue

            try:
                img = decode_texture(tex)
                out_path = out_dir / f"{_safe_filename(tex.name)}.png"
                img.save(out_path)
                print(f"  Extracted: {tex.name} ({tex.width}x{tex.height} "
                      f"{tex.format_name}) -> {out_path}")
                extracted += 1
            except tex_mod.TextureDecodeError as e:
                print(f"  Warning: cannot decode '{tex.name}': {e}",
                      file=sys.stderr)

    print(f"\n{extracted} texture(s) extracted.")


def _scan_bundles(args: argparse.Namespace) -> None:
    """Scan directory of .mtga bundles for matching textures."""
    scan_dir = Path(args.directory)
    pattern = args.pattern
    glob_pattern = "**/*.mtga" if args.recursive else "*.mtga"

    bundle_files = sorted(scan_dir.glob(glob_pattern))
    print(f"Scanning {len(bundle_files)} bundle(s) in {scan_dir}...")

    total_matches = 0
    for bundle_path in bundle_files:
        try:
            bnd = bundle_mod.parse_bundle(str(bundle_path))
        except Exception:
            continue

        for entry in _serialized_entries(bnd):
            try:
                entry_data = bundle_mod.get_entry_data(bnd, entry)
                sf = ser_mod.parse_serialized_file(entry_data)
            except Exception:
                continue

            for obj in sf.objects_by_class(CLASS_TEXTURE2D):
                try:
                    obj_data = sf.get_object_data(obj)
                    tex = tex_mod.read_texture2d(obj_data, sf.big_endian)
                except Exception:
                    continue

                if fnmatch.fnmatch(tex.name, pattern):
                    stream_flag = " [STREAM]" if tex.has_stream_data else ""
                    print(f"  {bundle_path.name}: {tex.name} "
                          f"({tex.width}x{tex.height} {tex.format_name})"
                          f"{stream_flag}")
                    total_matches += 1

    print(f"\n{total_matches} match(es) found.")


def _list_sprites(args: argparse.Namespace) -> None:
    """List all Sprite assets in a bundle file."""
    path = args.bundle
    try:
        bnd = bundle_mod.parse_bundle(path)
    except bundle_mod.BundleParseError as e:
        print(f"Error parsing bundle: {e}", file=sys.stderr)
        sys.exit(1)

    results = []
    for entry in _serialized_entries(bnd):
        try:
            entry_data = bundle_mod.get_entry_data(bnd, entry)
            sf = ser_mod.parse_serialized_file(entry_data)
        except Exception as e:
            print(f"  Warning: failed to parse entry '{entry.name}': {e}",
                  file=sys.stderr)
            continue

        for obj in sf.objects_by_class(CLASS_SPRITE):
            try:
                obj_data = sf.get_object_data(obj)
                spr = sprite_mod.read_sprite(obj_data, sf.big_endian)
                tr = spr.texture_rect
                info = {
                    "name": spr.name,
                    "texture_rect": f"{tr[0]:.0f},{tr[1]:.0f} {tr[2]:.0f}x{tr[3]:.0f}",
                    "texture_path_id": spr.texture_path_id,
                    "packed": spr.is_packed,
                    "rotation": spr.packing_rotation,
                    "entry": entry.name,
                }
                results.append(info)
            except sprite_mod.SpriteParseError as e:
                print(f"  Warning: failed to read sprite in '{entry.name}': {e}",
                      file=sys.stderr)

    if args.json:
        print(json.dumps(results, indent=2))
    else:
        if not results:
            print("No Sprite objects found.")
            return
        for r in results:
            packed_flag = " [PACKED]" if r["packed"] else ""
            rot_flag = f" rot={r['rotation']}" if r["rotation"] else ""
            print(f"  {r['name']:40s}  rect={r['texture_rect']:20s}  "
                  f"pathID={r['texture_path_id']}{packed_flag}{rot_flag}")


def _apply_packing_rotation(img, rotation: int):
    """Apply sprite packing rotation to a cropped image.

    Args:
        img: PIL Image to rotate.
        rotation: Packing rotation value from settingsRaw bits 2-5.

    Returns:
        Rotated PIL Image.
    """
    from PIL import Image

    if rotation == sprite_mod.PACK_ROTATION_FLIP_H:
        return img.transpose(Image.FLIP_LEFT_RIGHT)
    elif rotation == sprite_mod.PACK_ROTATION_FLIP_V:
        return img.transpose(Image.FLIP_TOP_BOTTOM)
    elif rotation == sprite_mod.PACK_ROTATION_ROTATE_180:
        return img.transpose(Image.ROTATE_180)
    elif rotation == sprite_mod.PACK_ROTATION_ROTATE_90:
        return img.transpose(Image.ROTATE_270)  # CW 90 = CCW 270
    return img


def _slice_sprites(args: argparse.Namespace) -> None:
    """Slice sprites from atlas textures as individual PNGs."""
    path = args.bundle
    pattern = args.pattern or "*"
    out_dir = Path(args.output) if args.output else Path(".")
    out_dir.mkdir(parents=True, exist_ok=True)

    try:
        bnd = bundle_mod.parse_bundle(path)
    except bundle_mod.BundleParseError as e:
        print(f"Error parsing bundle: {e}", file=sys.stderr)
        sys.exit(1)

    # First pass: collect Texture2D, Sprite, and SpriteAtlas objects
    tex_index: dict[int, tuple[tex_mod.Texture2D, ser_mod.SerializedFile]] = {}
    sprites: list[tuple[sprite_mod.Sprite, ser_mod.SerializedFile]] = []
    # Unified atlas lookup: render_data_key -> SpriteAtlasEntry
    atlas_map: dict[tuple[bytes, int], atlas_mod.SpriteAtlasEntry] = {}

    for entry in _serialized_entries(bnd):
        try:
            entry_data = bundle_mod.get_entry_data(bnd, entry)
            sf = ser_mod.parse_serialized_file(entry_data)
        except Exception as e:
            print(f"  Warning: skipping entry '{entry.name}': {e}",
                  file=sys.stderr)
            continue

        # Index textures by path_id
        for obj in sf.objects_by_class(CLASS_TEXTURE2D):
            try:
                obj_data = sf.get_object_data(obj)
                tex = tex_mod.read_texture2d(obj_data, sf.big_endian)
                tex_index[obj.path_id] = (tex, sf)
            except Exception:
                pass

        # Collect sprites
        for obj in sf.objects_by_class(CLASS_SPRITE):
            try:
                obj_data = sf.get_object_data(obj)
                spr = sprite_mod.read_sprite(obj_data, sf.big_endian)
                sprites.append((spr, sf))
            except sprite_mod.SpriteParseError as e:
                print(f"  Warning: failed to read sprite: {e}",
                      file=sys.stderr)

        # Parse SpriteAtlas objects into the lookup map
        for obj in sf.objects_by_class(CLASS_SPRITE_ATLAS):
            try:
                obj_data = sf.get_object_data(obj)
                entries = atlas_mod.read_sprite_atlas(obj_data, sf.big_endian)
                atlas_map.update(entries)
            except atlas_mod.SpriteAtlasParseError as e:
                print(f"  Warning: failed to read SpriteAtlas: {e}",
                      file=sys.stderr)

    if not sprites:
        print("No Sprite objects found.")
        return

    # Cache decoded atlas images to avoid re-decoding
    decoded_cache: dict[int, object] = {}  # path_id -> PIL Image
    extracted = 0

    for spr, sf in sprites:
        if not fnmatch.fnmatch(spr.name, pattern):
            continue

        # Resolve texture reference — direct PPtr or SpriteAtlas indirection
        tex_path_id = spr.texture_path_id
        texture_rect = spr.texture_rect
        packing_rotation = spr.packing_rotation

        if spr.texture_file_id != 0:
            print(f"  Warning: '{spr.name}' references texture in external file "
                  f"(fileID={spr.texture_file_id}), skipping",
                  file=sys.stderr)
            continue

        if tex_path_id == 0:
            # SpriteAtlas indirection — look up via render_data_key
            atlas_entry = atlas_map.get(spr.render_data_key)
            if atlas_entry is None:
                print(f"  Warning: '{spr.name}' has null texture and no "
                      f"SpriteAtlas entry found, skipping", file=sys.stderr)
                continue
            if atlas_entry.texture_file_id != 0:
                print(f"  Warning: '{spr.name}' atlas entry references "
                      f"external file, skipping", file=sys.stderr)
                continue
            tex_path_id = atlas_entry.texture_path_id
            texture_rect = atlas_entry.texture_rect
            packing_rotation = atlas_entry.packing_rotation

        if tex_path_id not in tex_index:
            print(f"  Warning: '{spr.name}' references texture pathID="
                  f"{tex_path_id} not found in bundle, skipping",
                  file=sys.stderr)
            continue

        # Decode atlas (cached)
        if tex_path_id not in decoded_cache:
            tex, _ = tex_index[tex_path_id]
            if tex.has_stream_data and len(tex.image_data) == 0:
                try:
                    tex.image_data = bundle_mod.resolve_stream_data(
                        bnd, tex.stream_path, tex.stream_offset, tex.stream_size
                    )
                except bundle_mod.BundleParseError as e:
                    print(f"  Warning: cannot resolve stream for atlas "
                          f"'{tex.name}': {e}", file=sys.stderr)
                    continue
            try:
                decoded_cache[tex_path_id] = decode_texture(tex)
            except tex_mod.TextureDecodeError as e:
                print(f"  Warning: cannot decode atlas '{tex.name}': {e}",
                      file=sys.stderr)
                continue

        atlas_img = decoded_cache[tex_path_id]
        atlas_w, atlas_h = atlas_img.size

        # Crop to textureRect — Unity coords have Y=0 at bottom,
        # PIL has Y=0 at top, so flip Y
        tx, ty, tw, th = texture_rect
        left = int(tx)
        # Unity Y is from bottom, PIL Y is from top
        bottom_y = int(ty)
        top_y = atlas_h - bottom_y - int(th)
        right = left + int(tw)
        bottom = top_y + int(th)

        # Clamp to atlas bounds
        left = max(0, left)
        top_y = max(0, top_y)
        right = min(atlas_w, right)
        bottom = min(atlas_h, bottom)

        if right <= left or bottom <= top_y:
            print(f"  Warning: '{spr.name}' has degenerate crop rect, skipping",
                  file=sys.stderr)
            continue

        sprite_img = atlas_img.crop((left, top_y, right, bottom))

        # Apply packing rotation if needed
        if packing_rotation != sprite_mod.PACK_ROTATION_NONE:
            sprite_img = _apply_packing_rotation(sprite_img, packing_rotation)

        out_path = out_dir / f"{_safe_filename(spr.name)}.png"
        sprite_img.save(out_path)
        print(f"  Sliced: {spr.name} ({int(tw)}x{int(th)}) -> {out_path}")
        extracted += 1

    print(f"\n{extracted} sprite(s) sliced.")


def main(argv: list[str] | None = None) -> None:
    """Entry point for the CLI."""
    parser = argparse.ArgumentParser(
        prog="unity_extract",
        description="Extract textures from Unity asset bundles (UnityFS format)",
    )
    sub = parser.add_subparsers(dest="command", required=True)

    # list
    p_list = sub.add_parser("list", help="List Texture2D assets in a bundle")
    p_list.add_argument("bundle", help="Path to .mtga bundle file")
    p_list.add_argument("--json", action="store_true", help="Output as JSON")

    # extract
    p_ext = sub.add_parser("extract", help="Extract textures as PNG files")
    p_ext.add_argument("bundle", help="Path to .mtga bundle file")
    p_ext.add_argument("--pattern", "-p", default="*",
                       help="Glob pattern for texture names (default: *)")
    p_ext.add_argument("--output", "-o", default=".",
                       help="Output directory (default: current dir)")

    # scan
    p_scan = sub.add_parser("scan", help="Scan bundles for matching textures")
    p_scan.add_argument("directory", help="Directory containing .mtga files")
    p_scan.add_argument("--pattern", "-p", required=True,
                        help="Glob pattern for texture names")
    p_scan.add_argument("--recursive", "-r", action="store_true",
                        help="Recurse into subdirectories")

    # list-sprites
    p_lspr = sub.add_parser("list-sprites",
                             help="List Sprite assets in a bundle")
    p_lspr.add_argument("bundle", help="Path to .mtga bundle file")
    p_lspr.add_argument("--json", action="store_true", help="Output as JSON")

    # slice
    p_slice = sub.add_parser("slice",
                              help="Slice sprites from atlas textures as PNGs")
    p_slice.add_argument("bundle", help="Path to .mtga bundle file")
    p_slice.add_argument("--pattern", "-p", default="*",
                         help="Glob pattern for sprite names (default: *)")
    p_slice.add_argument("--output", "-o", default=".",
                         help="Output directory (default: current dir)")

    args = parser.parse_args(argv)

    if args.command == "list":
        _list_textures(args)
    elif args.command == "extract":
        _extract_textures(args)
    elif args.command == "scan":
        _scan_bundles(args)
    elif args.command == "list-sprites":
        _list_sprites(args)
    elif args.command == "slice":
        _slice_sprites(args)
