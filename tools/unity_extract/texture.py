"""Texture2D field reading — parse Texture2D objects from serialized data.

Field layout is hardcoded for Unity 2021.3+ (MTGA's engine version).
All reads are sequential with explicit align4() calls after strings and bools.

Pixel decoders live in decoders.py (split for file size).
"""

from __future__ import annotations

import io
import struct
from dataclasses import dataclass
from enum import IntEnum

class TextureFormat(IntEnum):
    """Unity TextureFormat enum (subset of values we handle)."""

    Alpha8 = 1
    ARGB4444 = 2
    RGB24 = 3
    RGBA32 = 4
    ARGB32 = 5
    RGB565 = 7
    R16 = 9
    DXT1 = 10
    DXT5 = 12
    RGBA4444 = 13
    BGRA32 = 14
    RHalf = 15
    RGHalf = 16
    RGBAHalf = 17
    RFloat = 18
    RGFloat = 19
    RGBAFloat = 20
    R8 = 63
    ETC2_RGBA8 = 45
    BC7 = 25
    BC4 = 26
    BC5 = 27


class TextureDecodeError(Exception):
    """Raised when texture data cannot be decoded."""


@dataclass(slots=True)
class Texture2D:
    """Parsed Texture2D asset."""

    name: str
    width: int
    height: int
    texture_format: TextureFormat
    mip_count: int
    is_readable: bool
    image_count: int
    texture_dimension: int
    filter_mode: int
    aniso_level: int
    image_data: bytes
    # StreamData info (if pixel data is in external .resS file)
    stream_offset: int = 0
    stream_size: int = 0
    stream_path: str = ""

    @property
    def has_stream_data(self) -> bool:
        return self.stream_size > 0 and len(self.stream_path) > 0

    @property
    def format_name(self) -> str:
        try:
            return TextureFormat(self.texture_format).name
        except ValueError:
            return f"Unknown({self.texture_format})"


class _Reader:
    """Sequential binary reader with alignment support."""

    __slots__ = ("_f", "_endian")

    def __init__(self, data: bytes | memoryview, big_endian: bool = False) -> None:
        self._f = io.BytesIO(bytes(data) if isinstance(data, memoryview) else data)
        self._endian = ">" if big_endian else "<"

    def read(self, n: int) -> bytes:
        return self._f.read(n)

    def tell(self) -> int:
        return self._f.tell()

    def seek(self, pos: int) -> None:
        self._f.seek(pos)

    def align4(self) -> None:
        """Align read position to next 4-byte boundary."""
        pos = self._f.tell()
        aligned = (pos + 3) & ~3
        if aligned > pos:
            self._f.read(aligned - pos)

    def int32(self) -> int:
        return struct.unpack(f"{self._endian}i", self._f.read(4))[0]

    def uint32(self) -> int:
        return struct.unpack(f"{self._endian}I", self._f.read(4))[0]

    def int64(self) -> int:
        return struct.unpack(f"{self._endian}q", self._f.read(8))[0]

    def uint16(self) -> int:
        return struct.unpack(f"{self._endian}H", self._f.read(2))[0]

    def float32(self) -> float:
        return struct.unpack(f"{self._endian}f", self._f.read(4))[0]

    def bool8(self) -> bool:
        return struct.unpack("?", self._f.read(1))[0]

    def string(self) -> str:
        """Read Unity string: int32 length + utf8 bytes, then align4."""
        length = self.int32()
        if length <= 0:
            self.align4()
            return ""
        raw = self._f.read(length)
        self.align4()
        return raw.decode("utf-8", errors="replace")

    def pptr(self) -> tuple[int, int]:
        """Read PPtr: int32 fileID + int64 pathID."""
        file_id = self.int32()
        path_id = self.int64()
        return file_id, path_id

    def null_terminated_string(self) -> str:
        """Read null-terminated string (no length prefix, no align)."""
        chars = bytearray()
        while True:
            b = self.read(1)
            if not b or b == b"\x00":
                break
            chars.extend(b)
        return chars.decode("utf-8", errors="replace")

    def bytes_with_length(self) -> bytes:
        """Read int32 length + raw bytes."""
        length = self.int32()
        if length <= 0:
            return b""
        return self._f.read(length)


def read_texture2d(data: bytes | memoryview, big_endian: bool = False) -> Texture2D:
    """Parse a Texture2D object from its serialized data.

    Field layout follows Unity 2021.3 LTS Texture2D serialization order.
    This is a hardcoded sequential read — if Unity changes the layout,
    this will need updating.

    Args:
        data: Raw object data from SerializedFile.
        big_endian: Whether the serialized file uses big-endian.

    Returns:
        Parsed Texture2D with metadata and image data.

    Raises:
        TextureDecodeError: If fields don't validate.
    """
    r = _Reader(data, big_endian)

    # m_Name (string: int32 len + bytes + align4)
    name = r.string()

    # m_ForcedFallbackFormat (int32)
    r.int32()
    # m_DownscaleFallback (bool, NO align — meta flag 0x0000)
    r.bool8()
    # m_IsAlphaChannelOptional (bool, ALIGN — meta flag 0x4000)
    r.bool8()
    r.align4()

    # m_Width, m_Height
    width = r.int32()
    height = r.int32()

    # m_CompleteImageSize (uint32)
    r.uint32()

    # m_MipsStripped (int32)
    r.int32()

    # m_TextureFormat (int32)
    tex_format_raw = r.int32()
    try:
        tex_format = TextureFormat(tex_format_raw)
    except ValueError:
        tex_format = tex_format_raw  # Keep raw value

    # m_MipCount (int32)
    mip_count = r.int32()

    # m_IsReadable (bool, NO align — meta flag 0x0000)
    is_readable = r.bool8()
    # m_IsPreProcessed (bool, NO align — meta flag 0x0001)
    r.bool8()
    # m_IgnoreMipmapLimit (bool, ALIGN — meta flag 0x4000)
    r.bool8()
    r.align4()

    # m_MipmapLimitGroupName (string — includes its own align4)
    r.string()

    # m_StreamingMipmaps (bool, ALIGN — meta flag 0x4000)
    r.bool8()
    r.align4()

    # m_StreamingMipmapsPriority (int32)
    r.int32()

    # m_ImageCount (int32)
    image_count = r.int32()

    # m_TextureDimension (int32)
    texture_dimension = r.int32()

    # m_TextureSettings (GLTextureSettings struct)
    filter_mode = r.int32()   # m_FilterMode
    aniso_level = r.int32()   # m_Aniso
    r.float32()               # m_MipBias
    r.int32()                 # m_WrapU
    r.int32()                 # m_WrapV
    r.int32()                 # m_WrapW

    # m_LightmapFormat (int32)
    r.int32()

    # m_ColorSpace (int32)
    r.int32()

    # m_PlatformBlob (byte array: int32 len + bytes + align4)
    blob_len = r.int32()
    if blob_len > 0:
        r.read(blob_len)
        r.align4()

    # image data (int32 length + raw bytes)
    image_data = r.bytes_with_length()

    # m_StreamData
    stream_offset = r.int64() if r.tell() + 8 <= len(data) else 0
    stream_size = r.uint32() if r.tell() + 4 <= len(data) else 0
    stream_path = r.string() if r.tell() + 4 <= len(data) else ""

    # Validate
    if width < 1 or width > 16384 or height < 1 or height > 16384:
        raise TextureDecodeError(
            f"Invalid dimensions {width}x{height} for texture '{name}'"
        )

    return Texture2D(
        name=name,
        width=width,
        height=height,
        texture_format=tex_format,
        mip_count=mip_count,
        is_readable=is_readable,
        image_count=image_count,
        texture_dimension=texture_dimension,
        filter_mode=filter_mode,
        aniso_level=aniso_level,
        image_data=image_data,
        stream_offset=stream_offset,
        stream_size=stream_size,
        stream_path=stream_path,
    )
