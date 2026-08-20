"""Sprite field reading — parse Sprite objects from serialized data.

Field layout is hardcoded for Unity 2021.3+ (MTGA's engine version).
A Sprite doesn't contain pixel data — it references a Texture2D atlas
and specifies a crop rectangle within it.

Sprite → SpriteRenderData → PPtr<Texture2D> (atlas reference)
                           → textureRect (x, y, width, height)
"""

from __future__ import annotations

from dataclasses import dataclass

from .texture import _Reader


class SpriteParseError(Exception):
    """Raised when a Sprite cannot be parsed."""


@dataclass(slots=True)
class Sprite:
    """Parsed Sprite asset — references an atlas Texture2D with crop rect."""

    name: str
    rect: tuple[float, float, float, float]         # m_Rect (x, y, w, h)
    pivot: tuple[float, float]                       # m_Pivot
    texture_file_id: int                             # m_RD.texture PPtr fileID
    texture_path_id: int                             # m_RD.texture PPtr pathID
    render_data_key: tuple[bytes, int]                # m_RenderDataKey (GUID, second)
    texture_rect: tuple[float, float, float, float]  # m_RD.textureRect (crop)
    settings_raw: int                                # packed rotation/packing
    is_packed: bool                                  # bit 0 of settingsRaw
    packing_rotation: int                            # bits 2-5 of settingsRaw


# Packing rotation constants (from settingsRaw bits 2-5)
PACK_ROTATION_NONE = 0
PACK_ROTATION_FLIP_H = 1
PACK_ROTATION_FLIP_V = 2
PACK_ROTATION_ROTATE_180 = 3
PACK_ROTATION_ROTATE_90 = 4


def read_sprite(data: bytes | memoryview, big_endian: bool = False) -> Sprite:
    """Parse a Sprite object from its serialized data.

    Field layout follows Unity 2021.3 LTS Sprite serialization order.
    Only reads up to the fields we need (textureRect, settingsRaw) then stops.

    Args:
        data: Raw object data from SerializedFile.
        big_endian: Whether the serialized file uses big-endian.

    Returns:
        Parsed Sprite with atlas reference and crop rectangle.

    Raises:
        SpriteParseError: If fields don't validate or parsing fails.
    """
    r = _Reader(data, big_endian)

    try:
        # 1. m_Name (string: int32 len + bytes + align4)
        name = r.string()

        # 2. m_Rect (4x float32: x, y, w, h)
        rect = (r.float32(), r.float32(), r.float32(), r.float32())

        # 3. m_Offset (2x float32)
        r.float32()  # offset.x
        r.float32()  # offset.y

        # 4. m_Border (4x float32)
        r.float32()  # border.x
        r.float32()  # border.y
        r.float32()  # border.z
        r.float32()  # border.w

        # 5. m_PixelsToUnits (float32)
        r.float32()

        # 6. m_Pivot (2x float32)
        pivot = (r.float32(), r.float32())

        # 7. m_Extrude (uint32)
        r.uint32()

        # 8. m_IsPolygon (bool + align4)
        r.bool8()
        r.align4()

        # 9. m_RenderDataKey (16 bytes GUID + int64 second)
        rdk_guid = r.read(16)
        rdk_second = r.int64()

        # 10. m_AtlasTags (string array: int32 count + N strings)
        atlas_tag_count = r.int32()
        for _ in range(atlas_tag_count):
            r.string()

        # 11. m_SpriteAtlas PPtr (int32 fileID + int64 pathID)
        r.pptr()

        # 12. SpriteRenderData (m_RD)

        # 12a. texture PPtr — the atlas texture reference
        tex_file_id, tex_path_id = r.pptr()

        # 12b. alphaTexture PPtr
        r.pptr()

        # 12c. secondary textures (int32 count + per-entry)
        sec_tex_count = r.int32()
        for _ in range(sec_tex_count):
            r.pptr()    # texture PPtr
            r.string()  # name (length-prefixed + align4)

        # 12d. submeshes (int32 count + per-submesh data)
        submesh_count = r.int32()
        for _ in range(submesh_count):
            # firstByte (uint32), indexCount (uint32), topology (int32),
            # baseVertex (uint32), firstVertex (uint32), vertexCount (uint32)
            r.uint32()  # firstByte
            r.uint32()  # indexCount
            r.int32()   # topology
            r.uint32()  # baseVertex
            r.uint32()  # firstVertex
            r.uint32()  # vertexCount
            # AABB: center (3x float32) + extent (3x float32) = 24 bytes
            for _ in range(6):
                r.float32()

        # 12e. index buffer (int32 len + bytes + align4)
        idx_buf_len = r.int32()
        if idx_buf_len > 0:
            r.read(idx_buf_len)
            r.align4()

        # 12f. vertex data
        vertex_count = r.uint32()
        channel_count = r.int32()
        # channels: 4 bytes each (stream, offset, format, dimension)
        for _ in range(channel_count):
            r.read(4)
        # vertex data buffer (int32 len + bytes + align4)
        vert_data_len = r.int32()
        if vert_data_len > 0:
            r.read(vert_data_len)
            r.align4()

        # 12g. m_Bindpose — Matrix4x4 array (int32 count + count × 64 bytes)
        # Present in SpriteRenderData for Unity 2018+. Usually count=0 for
        # non-skinned sprites, but the int32 count field is always present.
        bindpose_count = r.int32()
        if bindpose_count > 0:
            r.read(bindpose_count * 64)  # 4x4 float32 matrix per bindpose

        # 12h. textureRect (4x float32: x, y, w, h)
        texture_rect = (r.float32(), r.float32(), r.float32(), r.float32())

        # 12i. textureRectOffset (2x float32)
        r.float32()
        r.float32()

        # 12j. atlasRectOffset (2x float32)
        r.float32()
        r.float32()

        # 12k. settingsRaw (uint32)
        settings_raw = r.uint32()

        # Decode settingsRaw
        is_packed = bool(settings_raw & 1)
        packing_rotation = (settings_raw >> 2) & 0xF

        # We have everything we need — skip uvTransform and downscaleMultiplier.

    except Exception as e:
        raise SpriteParseError(
            f"Failed to parse Sprite '{name if 'name' in dir() else '?'}' "
            f"at offset {r.tell()}: {e}"
        ) from e

    # Validate
    if texture_rect[2] < 0 or texture_rect[3] < 0:
        raise SpriteParseError(
            f"Invalid textureRect dimensions for '{name}': {texture_rect}"
        )

    return Sprite(
        name=name,
        rect=rect,
        pivot=pivot,
        texture_file_id=tex_file_id,
        texture_path_id=tex_path_id,
        render_data_key=(rdk_guid, rdk_second),
        texture_rect=texture_rect,
        settings_raw=settings_raw,
        is_packed=is_packed,
        packing_rotation=packing_rotation,
    )
