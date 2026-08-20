"""Pixel format decoders for Unity Texture2D assets.

Decodes raw pixel data from various GPU-native formats into RGBA byte arrays
suitable for PIL Image construction.

Supported formats:
  - RGBA32, ARGB32, RGB24, BGRA32 — direct byte reinterpretation
  - Alpha8, R8 — single-channel expansion
  - DXT1 (BC1) — 4-bit color with 1-bit alpha
  - DXT5 (BC3) — interpolated alpha + DXT1 color
  - BC7 (BPTC) — high-quality block compression (via texture2ddecoder)
  - BC4, BC5 — single/dual-channel block compression (via texture2ddecoder)
"""

from __future__ import annotations

import struct

from PIL import Image

try:
    import texture2ddecoder
    _HAS_T2D = True
except ImportError:
    _HAS_T2D = False

from .texture import Texture2D, TextureDecodeError, TextureFormat


def decode_texture(tex: Texture2D) -> Image.Image:
    """Decode a Texture2D's pixel data into a PIL Image.

    Args:
        tex: Parsed Texture2D with image_data populated.

    Returns:
        RGBA PIL Image (vertically flipped to standard orientation).

    Raises:
        TextureDecodeError: If the format is unsupported or data is wrong size.
    """
    if tex.has_stream_data and len(tex.image_data) == 0:
        raise TextureDecodeError(
            f"Texture '{tex.name}' uses external stream data "
            f"({tex.stream_path}) — not supported for extraction"
        )

    fmt = tex.texture_format
    w, h = tex.width, tex.height
    data = tex.image_data

    if fmt == TextureFormat.RGBA32:
        pixels = _decode_rgba32(data, w, h)
    elif fmt == TextureFormat.ARGB32:
        pixels = _decode_argb32(data, w, h)
    elif fmt == TextureFormat.RGB24:
        pixels = _decode_rgb24(data, w, h)
    elif fmt == TextureFormat.BGRA32:
        pixels = _decode_bgra32(data, w, h)
    elif fmt == TextureFormat.DXT1:
        pixels = _decode_dxt1(data, w, h)
    elif fmt == TextureFormat.DXT5:
        pixels = _decode_dxt5(data, w, h)
    elif fmt == TextureFormat.Alpha8:
        pixels = _decode_alpha8(data, w, h)
    elif fmt == TextureFormat.R8:
        pixels = _decode_r8(data, w, h)
    elif fmt == TextureFormat.BC7:
        pixels = _decode_bc7(data, w, h)
    elif fmt == TextureFormat.BC4:
        pixels = _decode_bc4(data, w, h)
    elif fmt == TextureFormat.BC5:
        pixels = _decode_bc5(data, w, h)
    else:
        name = tex.format_name
        raise TextureDecodeError(
            f"Unsupported texture format: {name} "
            f"(texture '{tex.name}', {w}x{h})"
        )

    img = Image.frombytes("RGBA", (w, h), pixels)
    # Unity stores textures bottom-up — flip vertically
    return img.transpose(Image.FLIP_TOP_BOTTOM)


# ---------------------------------------------------------------------------
# Simple format decoders
# ---------------------------------------------------------------------------

def _decode_rgba32(data: bytes, w: int, h: int) -> bytes:
    """RGBA32: 4 bytes per pixel, already in RGBA order."""
    expected = w * h * 4
    if len(data) < expected:
        raise TextureDecodeError(
            f"RGBA32 data too short: {len(data)} < {expected}"
        )
    return data[:expected]


def _decode_argb32(data: bytes, w: int, h: int) -> bytes:
    """ARGB32: 4 bytes per pixel, reorder to RGBA."""
    expected = w * h * 4
    if len(data) < expected:
        raise TextureDecodeError(
            f"ARGB32 data too short: {len(data)} < {expected}"
        )
    out = bytearray(expected)
    for i in range(0, expected, 4):
        a, r, g, b = data[i], data[i + 1], data[i + 2], data[i + 3]
        out[i] = r
        out[i + 1] = g
        out[i + 2] = b
        out[i + 3] = a
    return bytes(out)


def _decode_rgb24(data: bytes, w: int, h: int) -> bytes:
    """RGB24: 3 bytes per pixel, expand to RGBA with alpha=255."""
    expected = w * h * 3
    if len(data) < expected:
        raise TextureDecodeError(
            f"RGB24 data too short: {len(data)} < {expected}"
        )
    out = bytearray(w * h * 4)
    src_i = 0
    dst_i = 0
    for _ in range(w * h):
        out[dst_i] = data[src_i]
        out[dst_i + 1] = data[src_i + 1]
        out[dst_i + 2] = data[src_i + 2]
        out[dst_i + 3] = 255
        src_i += 3
        dst_i += 4
    return bytes(out)


def _decode_bgra32(data: bytes, w: int, h: int) -> bytes:
    """BGRA32: 4 bytes per pixel, reorder to RGBA."""
    expected = w * h * 4
    if len(data) < expected:
        raise TextureDecodeError(
            f"BGRA32 data too short: {len(data)} < {expected}"
        )
    out = bytearray(expected)
    for i in range(0, expected, 4):
        b, g, r, a = data[i], data[i + 1], data[i + 2], data[i + 3]
        out[i] = r
        out[i + 1] = g
        out[i + 2] = b
        out[i + 3] = a
    return bytes(out)


def _decode_alpha8(data: bytes, w: int, h: int) -> bytes:
    """Alpha8: 1 byte per pixel, white with variable alpha."""
    expected = w * h
    if len(data) < expected:
        raise TextureDecodeError(
            f"Alpha8 data too short: {len(data)} < {expected}"
        )
    out = bytearray(w * h * 4)
    for i in range(expected):
        a = data[i]
        out[i * 4] = 255
        out[i * 4 + 1] = 255
        out[i * 4 + 2] = 255
        out[i * 4 + 3] = a
    return bytes(out)


def _decode_r8(data: bytes, w: int, h: int) -> bytes:
    """R8: 1 byte per pixel, red channel only."""
    expected = w * h
    if len(data) < expected:
        raise TextureDecodeError(
            f"R8 data too short: {len(data)} < {expected}"
        )
    out = bytearray(w * h * 4)
    for i in range(expected):
        v = data[i]
        out[i * 4] = v
        out[i * 4 + 1] = 0
        out[i * 4 + 2] = 0
        out[i * 4 + 3] = 255
    return bytes(out)


# ---------------------------------------------------------------------------
# BC7 / BC4 / BC5 (via texture2ddecoder)
# ---------------------------------------------------------------------------

def _decode_bc7(data: bytes, w: int, h: int) -> bytes:
    """Decode BC7 (BPTC) via texture2ddecoder. Output is BGRA → convert to RGBA."""
    if not _HAS_T2D:
        raise TextureDecodeError(
            "BC7 decoding requires texture2ddecoder: pip install texture2ddecoder"
        )
    decoded = texture2ddecoder.decode_bc7(data, w, h)
    # texture2ddecoder outputs BGRA — swap to RGBA
    return _bgra_to_rgba(decoded, w, h)


def _decode_bc4(data: bytes, w: int, h: int) -> bytes:
    """Decode BC4 (single-channel) via texture2ddecoder."""
    if not _HAS_T2D:
        raise TextureDecodeError(
            "BC4 decoding requires texture2ddecoder: pip install texture2ddecoder"
        )
    decoded = texture2ddecoder.decode_bc4(data, w, h)
    return _bgra_to_rgba(decoded, w, h)


def _decode_bc5(data: bytes, w: int, h: int) -> bytes:
    """Decode BC5 (dual-channel) via texture2ddecoder."""
    if not _HAS_T2D:
        raise TextureDecodeError(
            "BC5 decoding requires texture2ddecoder: pip install texture2ddecoder"
        )
    decoded = texture2ddecoder.decode_bc5(data, w, h)
    return _bgra_to_rgba(decoded, w, h)


def _bgra_to_rgba(data: bytes, w: int, h: int) -> bytes:
    """Convert BGRA byte buffer to RGBA."""
    buf = bytearray(data)
    for i in range(0, w * h * 4, 4):
        buf[i], buf[i + 2] = buf[i + 2], buf[i]
    return bytes(buf)


# ---------------------------------------------------------------------------
# DXT / BC block-compressed decoders
# ---------------------------------------------------------------------------

def _rgb565_to_rgb(c: int) -> tuple[int, int, int]:
    """Unpack RGB565 to (R, G, B) with proper 5/6-bit expansion."""
    r = ((c >> 11) & 0x1F) * 255 // 31
    g = ((c >> 5) & 0x3F) * 255 // 63
    b = (c & 0x1F) * 255 // 31
    return r, g, b


def _decode_dxt1_block(block: bytes) -> list[tuple[int, int, int, int]]:
    """Decode one 8-byte DXT1 (BC1) block into 16 RGBA pixels."""
    c0 = struct.unpack_from("<H", block, 0)[0]
    c1 = struct.unpack_from("<H", block, 2)[0]
    indices = struct.unpack_from("<I", block, 4)[0]

    r0, g0, b0 = _rgb565_to_rgb(c0)
    r1, g1, b1 = _rgb565_to_rgb(c1)

    if c0 > c1:
        colors = [
            (r0, g0, b0, 255),
            (r1, g1, b1, 255),
            ((2 * r0 + r1) // 3, (2 * g0 + g1) // 3, (2 * b0 + b1) // 3, 255),
            ((r0 + 2 * r1) // 3, (g0 + 2 * g1) // 3, (b0 + 2 * b1) // 3, 255),
        ]
    else:
        colors = [
            (r0, g0, b0, 255),
            (r1, g1, b1, 255),
            ((r0 + r1) // 2, (g0 + g1) // 2, (b0 + b1) // 2, 255),
            (0, 0, 0, 0),  # transparent black
        ]

    pixels = []
    for i in range(16):
        idx = (indices >> (2 * i)) & 0x3
        pixels.append(colors[idx])
    return pixels


def _decode_dxt5_alpha_block(block: bytes) -> list[int]:
    """Decode 8-byte DXT5 alpha block into 16 alpha values."""
    a0 = block[0]
    a1 = block[1]

    # 48-bit index data (6 bytes, 16 x 3-bit indices)
    bits = 0
    for i in range(2, 8):
        bits |= block[i] << (8 * (i - 2))

    if a0 > a1:
        alphas = [
            a0, a1,
            (6 * a0 + 1 * a1) // 7,
            (5 * a0 + 2 * a1) // 7,
            (4 * a0 + 3 * a1) // 7,
            (3 * a0 + 4 * a1) // 7,
            (2 * a0 + 5 * a1) // 7,
            (1 * a0 + 6 * a1) // 7,
        ]
    else:
        alphas = [
            a0, a1,
            (4 * a0 + 1 * a1) // 5,
            (3 * a0 + 2 * a1) // 5,
            (2 * a0 + 3 * a1) // 5,
            (1 * a0 + 4 * a1) // 5,
            0,
            255,
        ]

    result = []
    for i in range(16):
        idx = (bits >> (3 * i)) & 0x7
        result.append(alphas[idx])
    return result


def _decode_dxt1(data: bytes, w: int, h: int) -> bytes:
    """Decode DXT1 (BC1) compressed texture into RGBA bytes."""
    bw = (w + 3) // 4
    bh = (h + 3) // 4
    expected = bw * bh * 8
    if len(data) < expected:
        raise TextureDecodeError(
            f"DXT1 data too short: {len(data)} < {expected}"
        )

    out = bytearray(w * h * 4)
    block_idx = 0

    for by in range(bh):
        for bx in range(bw):
            offset = block_idx * 8
            pixels = _decode_dxt1_block(data[offset : offset + 8])
            block_idx += 1

            for py in range(4):
                for px in range(4):
                    x = bx * 4 + px
                    y = by * 4 + py
                    if x < w and y < h:
                        r, g, b, a = pixels[py * 4 + px]
                        oi = (y * w + x) * 4
                        out[oi] = r
                        out[oi + 1] = g
                        out[oi + 2] = b
                        out[oi + 3] = a

    return bytes(out)


def _decode_dxt5(data: bytes, w: int, h: int) -> bytes:
    """Decode DXT5 (BC3) compressed texture into RGBA bytes.

    Each 16-byte block: 8 bytes alpha + 8 bytes DXT1 color.
    """
    bw = (w + 3) // 4
    bh = (h + 3) // 4
    expected = bw * bh * 16
    if len(data) < expected:
        raise TextureDecodeError(
            f"DXT5 data too short: {len(data)} < {expected}"
        )

    out = bytearray(w * h * 4)
    block_idx = 0

    for by in range(bh):
        for bx in range(bw):
            offset = block_idx * 16
            alpha_values = _decode_dxt5_alpha_block(data[offset : offset + 8])
            color_pixels = _decode_dxt1_block(data[offset + 8 : offset + 16])
            block_idx += 1

            for py in range(4):
                for px in range(4):
                    x = bx * 4 + px
                    y = by * 4 + py
                    if x < w and y < h:
                        pi = py * 4 + px
                        r, g, b, _ = color_pixels[pi]
                        a = alpha_values[pi]
                        oi = (y * w + x) * 4
                        out[oi] = r
                        out[oi + 1] = g
                        out[oi + 2] = b
                        out[oi + 3] = a

    return bytes(out)
