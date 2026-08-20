"""SpriteAtlas parsing — resolve indirect sprite texture references.

SpriteAtlas (classID 687078895) contains a m_RenderDataMap that maps
render data keys (GUID + int64) to atlas texture references and crop rects.
Sprites with null texture PPtrs in m_RD use this indirection — their
m_RenderDataKey is the lookup key into the atlas's map.

Only parses m_RenderDataMap — skips m_PackedSprites, m_PackedSpriteNamesToIndex,
and trailing fields (m_Tag, m_IsVariant) since they're not needed for resolution.

Format verified by byte-level tracing of MTGA Atlas_NavBar_*.mtga bundles.
"""

from __future__ import annotations

from dataclasses import dataclass

from .texture import _Reader


class SpriteAtlasParseError(Exception):
    """Raised when a SpriteAtlas cannot be parsed."""


@dataclass(slots=True)
class SpriteAtlasEntry:
    """One entry from SpriteAtlas.m_RenderDataMap."""

    texture_file_id: int
    texture_path_id: int
    texture_rect: tuple[float, float, float, float]  # x, y, w, h in atlas
    settings_raw: int
    is_packed: bool
    packing_rotation: int


def read_sprite_atlas(
    data: bytes | memoryview, big_endian: bool = False
) -> dict[tuple[bytes, int], SpriteAtlasEntry]:
    """Parse a SpriteAtlas and return its renderDataKey -> entry map.

    Args:
        data: Raw object data from SerializedFile.
        big_endian: Whether the serialized file uses big-endian.

    Returns:
        Dict mapping (GUID bytes[16], int64 second) to SpriteAtlasEntry.

    Raises:
        SpriteAtlasParseError: If parsing fails.
    """
    r = _Reader(data, big_endian)

    try:
        # 1. m_Name (string)
        r.string()

        # 2. m_PackedSprites (Array<PPtr<Sprite>>)
        packed_count = r.int32()
        for _ in range(packed_count):
            r.pptr()  # PPtr: int32 fileID + int64 pathID

        # 3. m_PackedSpriteNamesToIndex (Array<string>)
        names_count = r.int32()
        for _ in range(names_count):
            r.string()

        # 4. m_RenderDataMap (Map: int32 count + per-entry)
        map_count = r.int32()

        result: dict[tuple[bytes, int], SpriteAtlasEntry] = {}
        for _ in range(map_count):
            # Key: 16 bytes GUID + int64 second
            guid = r.read(16)
            second = r.int64()

            # Value (84 bytes + variable secondary textures):
            # PPtr texture (12)
            tex_file_id, tex_path_id = r.pptr()
            # PPtr alphaTexture (12)
            r.pptr()
            # Rectf textureRect (16): x, y, w, h
            tex_rect = (r.float32(), r.float32(), r.float32(), r.float32())
            # Vector2f textureRectOffset (8)
            r.float32()
            r.float32()
            # Vector2f atlasRectOffset (8)
            r.float32()
            r.float32()
            # Vector4f uvTransform (16)
            r.float32()
            r.float32()
            r.float32()
            r.float32()
            # float downscaleMultiplier (4)
            r.float32()
            # uint32 settingsRaw (4)
            settings_raw = r.uint32()
            # int32 secondaryTextures count + per-entry: PPtr + string
            sec_count = r.int32()
            for _ in range(sec_count):
                r.pptr()
                r.string()

            is_packed = bool(settings_raw & 1)
            packing_rotation = (settings_raw >> 2) & 0xF

            result[(guid, second)] = SpriteAtlasEntry(
                texture_file_id=tex_file_id,
                texture_path_id=tex_path_id,
                texture_rect=tex_rect,
                settings_raw=settings_raw,
                is_packed=is_packed,
                packing_rotation=packing_rotation,
            )

    except Exception as e:
        raise SpriteAtlasParseError(
            f"Failed to parse SpriteAtlas at offset {r.tell()}: {e}"
        ) from e

    return result
