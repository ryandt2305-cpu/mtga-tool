"""Parse Unity SerializedFile format found inside asset bundles.

Each directory entry in a UnityFS bundle typically contains a SerializedFile.
This module parses the header, type table (classIDs), and object info table
to enumerate the assets (Texture2D, Sprite, etc.) within.

Supports format versions 14–22 (covers all modern Unity builds).

References:
  - UnityPy / AssetStudio SerializedFile parsers
  - Unity source: Runtime/Serialize/SerializedFile.cpp
"""

from __future__ import annotations

import io
import struct
from dataclasses import dataclass, field


class SerializedFileError(Exception):
    """Raised when a serialized file cannot be parsed."""


# Unity classID constants we care about
CLASS_TEXTURE2D = 28
CLASS_SPRITE = 213
CLASS_TEXT_ASSET = 49
CLASS_MONO_BEHAVIOUR = 114
CLASS_SPRITE_ATLAS = 687078895


@dataclass(frozen=True, slots=True)
class TypeInfo:
    """A type entry from the serialized file's type table."""

    class_id: int
    is_stripped: bool
    script_type_index: int  # -1 if not a script


@dataclass(frozen=True, slots=True)
class ObjectInfo:
    """Metadata for one serialized object."""

    path_id: int
    data_offset: int  # relative to data region start
    data_size: int
    class_id: int
    type_index: int


@dataclass(slots=True)
class SerializedFile:
    """Parsed serialized file with object table."""

    format_version: int
    unity_version: str
    platform: int
    big_endian: bool
    types: list[TypeInfo]
    objects: list[ObjectInfo]
    data: memoryview  # raw data backing; object offsets are relative to data_offset
    data_offset: int  # absolute offset of data region within the file

    def get_object_data(self, obj: ObjectInfo) -> memoryview:
        """Get raw bytes for a specific object."""
        start = obj.data_offset
        return self.data[start : start + obj.data_size]

    def objects_by_class(self, class_id: int) -> list[ObjectInfo]:
        """Return all objects with the given classID."""
        return [o for o in self.objects if o.class_id == class_id]

    def object_by_path_id(self, path_id: int) -> ObjectInfo | None:
        """Find an object by its path ID."""
        for obj in self.objects:
            if obj.path_id == path_id:
                return obj
        return None


def parse_serialized_file(data: bytes | memoryview) -> SerializedFile:
    """Parse a SerializedFile from raw bytes.

    Args:
        data: Raw bytes of the serialized file (from bundle entry).

    Returns:
        SerializedFile with object table and data reference.

    Raises:
        SerializedFileError: If the data cannot be parsed.
    """
    if isinstance(data, memoryview):
        raw = bytes(data)
    else:
        raw = data

    f = io.BytesIO(raw)
    mv = memoryview(raw)

    # --- Header (always big-endian for the fixed fields) ---
    header_be = struct.Struct(">IIIi")
    if len(raw) < header_be.size:
        raise SerializedFileError("File too small for header")

    metadata_size, file_size_32, format_version, data_offset_32 = header_be.unpack(
        f.read(header_be.size)
    )

    if format_version < 14 or format_version > 22:
        raise SerializedFileError(
            f"Unsupported format version {format_version} (need 14-22)"
        )

    file_size = file_size_32
    data_offset = data_offset_32

    # Endianness (version >= 9) — must read BEFORE v22 extended header
    big_endian = False
    if format_version >= 9:
        endian_byte = struct.unpack("B", f.read(1))[0]
        big_endian = endian_byte != 0
        f.read(3)  # reserved padding

    # Version >= 22: extended header with 64-bit fields follows endianness
    if format_version >= 22:
        metadata_size = struct.unpack(">I", f.read(4))[0]
        file_size = struct.unpack(">q", f.read(8))[0]
        data_offset = struct.unpack(">q", f.read(8))[0]
        f.read(8)  # unknown field

    endian = ">" if big_endian else "<"

    # --- Unity version string ---
    unity_version = _read_cstr(f)

    # --- Platform ---
    (platform,) = struct.unpack(f"{endian}I", f.read(4))

    # --- Type trees ---
    (has_type_trees,) = struct.unpack("?", f.read(1))
    (type_count,) = struct.unpack(f"{endian}I", f.read(4))

    types: list[TypeInfo] = []
    for _ in range(type_count):
        type_info = _read_type_entry(f, format_version, endian, has_type_trees)
        types.append(type_info)

    # --- Object info table ---
    (object_count,) = struct.unpack(f"{endian}I", f.read(4))
    if object_count < 0 or object_count > 500000:
        raise SerializedFileError(f"Unreasonable object count: {object_count}")

    objects: list[ObjectInfo] = []
    for _ in range(object_count):
        obj = _read_object_info(f, format_version, endian, types)
        objects.append(obj)

    return SerializedFile(
        format_version=format_version,
        unity_version=unity_version,
        platform=platform,
        big_endian=big_endian,
        types=types,
        objects=objects,
        data=mv[data_offset:],
        data_offset=data_offset,
    )


def _read_cstr(f: io.BytesIO) -> str:
    """Read null-terminated string."""
    chars = bytearray()
    while True:
        b = f.read(1)
        if not b or b == b"\x00":
            break
        chars.extend(b)
    return chars.decode("utf-8", errors="replace")


def _read_type_entry(
    f: io.BytesIO, version: int, endian: str, has_trees: bool
) -> TypeInfo:
    """Read one type table entry."""
    (class_id,) = struct.unpack(f"{endian}i", f.read(4))

    is_stripped = False
    if version >= 16:
        (is_stripped,) = struct.unpack("?", f.read(1))

    script_type_index = -1
    if version >= 17:
        (script_type_index,) = struct.unpack(f"{endian}h", f.read(2))

    # Type hash
    if version >= 13:
        if (version < 16 and class_id < 0) or (version >= 16 and class_id == 114):
            f.read(32)  # script_id (16 bytes) + old_type_hash (16 bytes)
        else:
            f.read(16)  # old_type_hash only

    # Type tree nodes (skip if present)
    if has_trees:
        _skip_type_tree(f, version, endian)

    # Type dependencies (version >= 21): int32 count + count * int32
    if version >= 21:
        (dep_count,) = struct.unpack(f"{endian}I", f.read(4))
        if dep_count > 0:
            f.read(dep_count * 4)

    return TypeInfo(class_id=class_id, is_stripped=is_stripped,
                    script_type_index=script_type_index)


def _skip_type_tree(f: io.BytesIO, version: int, endian: str) -> None:
    """Skip type tree node data — we don't need it for extraction."""
    if version >= 21:
        # "Blob" format: node_count, string_buffer_size, then flat arrays
        node_count, str_buf_size = struct.unpack(f"{endian}II", f.read(8))
        # Node size: 32 bytes for version >= 19 (includes uint64 RefTypeHash),
        # 24 bytes for older versions
        node_size = 32 if version >= 19 else 24
        f.read(node_count * node_size)
        # String buffer
        f.read(str_buf_size)
    else:
        # Recursive format: read node count then skip
        # This is complex; for versions < 21 we use a simpler recursive skip
        _skip_type_tree_recursive(f, version, endian)


def _skip_type_tree_recursive(f: io.BytesIO, version: int, endian: str) -> None:
    """Skip a recursive type tree (versions < 21)."""
    # type, name (strings), size, index, arrayFlag, version, metaFlags
    # Plus children count → recurse
    # Version field: string, string, int32, int32, int32, int32, int32
    # then int32 child count
    _read_cstr(f)  # type
    _read_cstr(f)  # name
    f.read(4 * 5)  # size, index, arrayFlag, version, metaFlags
    (child_count,) = struct.unpack(f"{endian}I", f.read(4))
    for _ in range(child_count):
        _skip_type_tree_recursive(f, version, endian)


def _read_object_info(
    f: io.BytesIO, version: int, endian: str, types: list[TypeInfo]
) -> ObjectInfo:
    """Read one object info entry."""
    # Align to 4 bytes before each object info (version >= 14)
    if version >= 14:
        pos = f.tell()
        aligned = (pos + 3) & ~3
        if aligned > pos:
            f.read(aligned - pos)

    # path_id: int64 for version >= 14
    if version >= 14:
        (path_id,) = struct.unpack(f"{endian}q", f.read(8))
    else:
        (path_id,) = struct.unpack(f"{endian}i", f.read(4))

    # Byte offset (version >= 22: uint64, else uint32)
    if version >= 22:
        (byte_start,) = struct.unpack(f"{endian}q", f.read(8))
    else:
        (byte_start,) = struct.unpack(f"{endian}I", f.read(4))

    # Size
    (byte_size,) = struct.unpack(f"{endian}I", f.read(4))

    # Type index (version >= 16) or classID + type directly
    if version >= 16:
        (type_index,) = struct.unpack(f"{endian}I", f.read(4))
        class_id = types[type_index].class_id if type_index < len(types) else -1
    else:
        (type_index,) = struct.unpack(f"{endian}I", f.read(4))
        (class_id,) = struct.unpack(f"{endian}h", f.read(2))
        f.read(2)  # script_type_index (int16)

    # Stripped flag (version == 15 or 16)
    if version == 15 or version == 16:
        f.read(1)  # is_destroyed (uint8)

    return ObjectInfo(
        path_id=path_id,
        data_offset=byte_start,
        data_size=byte_size,
        class_id=class_id,
        type_index=type_index,
    )
