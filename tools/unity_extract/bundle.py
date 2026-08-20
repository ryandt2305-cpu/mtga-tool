"""Parse UnityFS asset bundle container format.

UnityFS is the outermost layer of Unity's asset bundle format.
Structure:
  1. Header (big-endian): signature, versions, sizes, flags
  2. Compressed block info (describes data blocks + directory)
  3. Compressed data blocks (concatenated = raw serialized files)

References:
  - https://docs.unity3d.com/Manual/AssetBundles-FileFormat.html
  - UnityPy / AssetStudio source code
"""

from __future__ import annotations

import io
import lzma
import struct
from dataclasses import dataclass, field
from enum import IntEnum

import lz4.block


class CompressionType(IntEnum):
    NONE = 0
    LZMA = 1
    LZ4 = 2
    LZ4HC = 3


class BundleParseError(Exception):
    """Raised when a bundle file cannot be parsed."""


@dataclass(frozen=True, slots=True)
class BlockInfo:
    """One compressed data block within the bundle."""

    uncompressed_size: int
    compressed_size: int
    flags: int

    @property
    def compression(self) -> CompressionType:
        return CompressionType(self.flags & 0x3F)


@dataclass(frozen=True, slots=True)
class DirectoryEntry:
    """One file inside the bundle (usually a SerializedFile)."""

    offset: int
    size: int
    flags: int
    name: str


@dataclass(slots=True)
class AssetBundle:
    """Parsed UnityFS bundle with decompressed content."""

    signature: str
    stream_version: int
    unity_version: str
    generator_version: str
    file_size: int
    entries: list[DirectoryEntry]
    data: bytes  # decompressed concatenation of all content blocks


def _read_cstr(f: io.BufferedIOBase) -> str:
    """Read a null-terminated string."""
    chars = bytearray()
    while True:
        b = f.read(1)
        if not b or b == b"\x00":
            break
        chars.extend(b)
    return chars.decode("utf-8", errors="replace")


def _decompress_block(data: bytes, compression: CompressionType,
                      uncompressed_size: int) -> bytes:
    """Decompress a single block."""
    if compression == CompressionType.NONE:
        return data
    if compression in (CompressionType.LZ4, CompressionType.LZ4HC):
        return lz4.block.decompress(data, uncompressed_size=uncompressed_size)
    if compression == CompressionType.LZMA:
        # Unity LZMA blocks: raw LZMA stream with 5-byte props + 8-byte size
        # prefix matching python-lzma's FORMAT_ALONE expectations.
        return lzma.decompress(data, format=lzma.FORMAT_AUTO)
    raise BundleParseError(f"Unknown compression type: {compression}")


def _parse_block_info(raw: bytes) -> tuple[list[BlockInfo], list[DirectoryEntry]]:
    """Parse the decompressed block/directory info structure.

    Layout (big-endian):
      16 bytes: hash (unused)
      int32: block_count
      per block: uint32 uncompressed_size, uint32 compressed_size, uint16 flags
      int32: directory_count
      per entry: int64 offset, int64 size, uint32 flags, null-terminated name
    """
    r = io.BytesIO(raw)

    # Skip 16-byte hash
    r.read(16)

    (block_count,) = struct.unpack(">i", r.read(4))
    if block_count < 0 or block_count > 65536:
        raise BundleParseError(f"Unreasonable block count: {block_count}")

    blocks: list[BlockInfo] = []
    for _ in range(block_count):
        u_size, c_size, flags = struct.unpack(">IIH", r.read(10))
        blocks.append(BlockInfo(u_size, c_size, flags))

    (dir_count,) = struct.unpack(">i", r.read(4))
    if dir_count < 0 or dir_count > 65536:
        raise BundleParseError(f"Unreasonable directory count: {dir_count}")

    entries: list[DirectoryEntry] = []
    for _ in range(dir_count):
        offset, size, flags = struct.unpack(">qqI", r.read(20))
        name = _read_cstr(r)
        entries.append(DirectoryEntry(offset, size, flags, name))

    return blocks, entries


def parse_bundle(source: str | bytes | io.BufferedIOBase) -> AssetBundle:
    """Parse a UnityFS bundle file.

    Args:
        source: File path, raw bytes, or file-like object.

    Returns:
        AssetBundle with decompressed data and directory entries.

    Raises:
        BundleParseError: If the file is not a valid UnityFS bundle.
    """
    if isinstance(source, (str,)):
        with open(source, "rb") as fh:
            return _parse_bundle_stream(fh)
    if isinstance(source, bytes):
        return _parse_bundle_stream(io.BytesIO(source))
    return _parse_bundle_stream(source)


def _parse_bundle_stream(f: io.BufferedIOBase) -> AssetBundle:
    """Internal: parse from an open binary stream."""

    # --- Header (big-endian) ---
    signature = _read_cstr(f)
    if signature != "UnityFS":
        raise BundleParseError(
            f"Not a UnityFS bundle (signature: {signature!r})"
        )

    (stream_version,) = struct.unpack(">I", f.read(4))
    unity_version = _read_cstr(f)
    generator_version = _read_cstr(f)

    (file_size,) = struct.unpack(">q", f.read(8))
    (compressed_block_info_size,) = struct.unpack(">I", f.read(4))
    (uncompressed_block_info_size,) = struct.unpack(">I", f.read(4))
    (flags,) = struct.unpack(">I", f.read(4))

    block_info_compression = CompressionType(flags & 0x3F)
    block_info_at_end = bool(flags & 0x80)

    # --- Read block info ---
    if block_info_at_end:
        current_pos = f.tell()
        f.seek(-compressed_block_info_size, 2)
        compressed_info = f.read(compressed_block_info_size)
        f.seek(current_pos)
    else:
        # Stream version >= 7 aligns to 16 bytes after header
        if stream_version >= 7:
            pos = f.tell()
            aligned = (pos + 15) & ~15
            if aligned > pos:
                f.read(aligned - pos)
        compressed_info = f.read(compressed_block_info_size)

    # Decompress block info
    raw_info = _decompress_block(
        compressed_info, block_info_compression, uncompressed_block_info_size
    )

    blocks, entries = _parse_block_info(raw_info)

    # --- Read and decompress content blocks ---
    # Align to 16 bytes before data blocks (required for version >= 7)
    if not block_info_at_end and stream_version >= 7:
        pos = f.tell()
        aligned = (pos + 15) & ~15
        if aligned > pos:
            f.read(aligned - pos)

    data_parts: list[bytes] = []
    for block in blocks:
        compressed_data = f.read(block.compressed_size)
        if len(compressed_data) < block.compressed_size:
            raise BundleParseError(
                f"Truncated block: expected {block.compressed_size} bytes, "
                f"got {len(compressed_data)}"
            )
        decompressed = _decompress_block(
            compressed_data, block.compression, block.uncompressed_size
        )
        data_parts.append(decompressed)

    full_data = b"".join(data_parts)

    return AssetBundle(
        signature=signature,
        stream_version=stream_version,
        unity_version=unity_version,
        generator_version=generator_version,
        file_size=file_size,
        entries=entries,
        data=full_data,
    )


def get_entry_data(bundle: AssetBundle, entry: DirectoryEntry) -> memoryview:
    """Get the raw data for a directory entry.

    Returns a memoryview into the bundle's decompressed data.
    """
    return memoryview(bundle.data)[entry.offset : entry.offset + entry.size]


def resolve_stream_data(
    bundle: AssetBundle, stream_path: str, offset: int, size: int
) -> bytes:
    """Read texture pixel data from a .resS stream entry in the bundle.

    Args:
        bundle: Parsed AssetBundle containing both serialized and .resS entries.
        stream_path: Stream path from Texture2D (e.g. "archive:/CAB-.../CAB-...resS").
        offset: Byte offset within the stream entry.
        size: Number of bytes to read.

    Returns:
        Raw pixel data bytes.

    Raises:
        BundleParseError: If the stream entry cannot be found.
    """
    # Extract the entry name from the stream path
    # Format: "archive:/CAB-xxx/CAB-xxx.resS" → "CAB-xxx.resS"
    entry_name = stream_path.rsplit("/", 1)[-1] if "/" in stream_path else stream_path

    for entry in bundle.entries:
        if entry.name == entry_name:
            data = bundle.data[entry.offset + offset : entry.offset + offset + size]
            return bytes(data)

    # Fallback: try matching by suffix
    for entry in bundle.entries:
        if entry.name.endswith(".resS") or entry.name.endswith(".resource"):
            data = bundle.data[entry.offset + offset : entry.offset + offset + size]
            return bytes(data)

    raise BundleParseError(
        f"Stream entry '{entry_name}' not found in bundle "
        f"(entries: {[e.name for e in bundle.entries]})"
    )
