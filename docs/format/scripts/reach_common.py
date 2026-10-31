# Shared read-only helpers for the MCC Halo: Reach (U13) probes. Reach's
# cache file has Halo 3's header fields in a 40 KiB header, and its packed
# tag-data pointers are offset by 0x50000000 (docs/format/reach.md 1), so
# this is h3_common.Map with that one change and Reach's maps folder.
# Use:  sys.path.insert(0, os.path.dirname(__file__)); from reach_common import Map
# Never writes anything.
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import h3_common  # noqa: E402

MAPS = os.environ.get(
    "HALOREACH_MAPS",
    r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\haloreach\maps",
)
POINTER_BASE = 0x50000000

MP_MAPS = [
    "dlc_slayer", "cex_beaver_creek", "50_panopticon", "70_boneyard", "cex_headlong",
    "dlc_invasion", "condemned", "45_launch_station", "forge_halo", "trainingpreserve",
    "cex_hangemhigh", "cex_damnation", "30_settlement", "52_ivory_tower", "cex_timberland",
    "cex_prisoner", "35_island", "20_sword_slayer", "dlc_medium", "45_aftship",
]


class Map(h3_common.Map):
    def __init__(self, name_or_path):
        path = name_or_path
        if not os.path.exists(path):
            path = os.path.join(MAPS, name_or_path + ".map")
        super().__init__(path)

    def ptr32(self, p):
        return self.vaddr(((p & 0xFFFFFFFF) << 2) + POINTER_BASE)

    # The resource system (docs/format/resources.md; the same layout in Reach):
    # zone + 0x64 entries (64 bytes), zone + 0x148 fixup data; play + 0x0C
    # shared caches, + 0x18 pages (88 bytes), + 0x3C segments (16 bytes).
    def resource(self, rid, data=True):
        """(entry fields, definition bytes, fixup offsets, data bytes or None)."""
        if not hasattr(self, "_zone"):
            self._zone = self.meta(self.find("zone"))
        zone = self._zone
        n, first = self.block(zone + 0x64)
        e = first + 64 * (rid & 0xFFFF)
        salt = self.u16(e + 0x10)
        fo, fs = self.u32(e + 0x14), self.u32(e + 0x18)
        seg = self.i16(e + 0x22)
        fix_size = self.u32(zone + 0x148)
        fix_at = self.ptr32(self.u32(zone + 0x154))
        definition = bytes(self.m[fix_at + fo:fix_at + fo + fs])
        fixups = [self.u32(o + 4) & 0x0FFFFFFF for o in self.elements(e + 0x28, 8)]
        data = self.segment_data(seg) if seg >= 0 and data else None
        return dict(salt=salt, segment=seg, type=self.i8(e + 0x12)), definition, fixups, data

    def segment_data(self, seg, secondary=False):
        import zlib
        play_i = self.find("play")
        play = self.meta(play_i) if self.tags[play_i][1] else self.meta(self.find("zone")) + 0x1C
        segs = self.elements(play + 0x3C, 16)
        s = segs[seg]
        pp, sp, po, so = self.i16(s), self.i16(s + 2), self.i32(s + 4), self.i32(s + 8)
        page, off = (sp, so) if (secondary or sp >= 0) and sp >= 0 else (pp, po)
        pages = self.elements(play + 0x18, 88)
        g = pages[page]
        cache_index, data_off = self.i16(g + 4), self.i32(g + 8)
        csize, dsize = self.u32(g + 0xC), self.u32(g + 0x10)
        if data_off < 0 or csize == 0:
            if page == sp and pp >= 0:
                return self.segment_data_page(play, pp, po)
            return None
        return self.segment_data_page(play, page, off)

    def segment_data_page(self, play, page, off):
        import zlib
        pages = self.elements(play + 0x18, 88)
        g = pages[page]
        cache_index, data_off = self.i16(g + 4), self.i32(g + 8)
        csize, dsize = self.u32(g + 0xC), self.u32(g + 0x10)
        src = self
        if cache_index >= 0:
            name = self.cstr(self.elements(play + 0x0C, 264)[cache_index], 32).split("\\")[-1]
            src = _shared(os.path.join(os.path.dirname(self.path), name))
        key = (src.path, page)
        cache = _PAGES
        if key not in cache:
            base = (src.u32(0x4D0) + src.u32(0x4DC + 8)) & 0xFFFFFFFF
            raw = bytes(src.m[base + data_off:base + data_off + csize])
            if len(cache) > 8:
                cache.clear()
            cache[key] = raw if csize == dsize else zlib.decompress(raw, -15)
        return memoryview(cache[key])[off:]


_SHARED = {}
_PAGES = {}


class _Raw:
    """A shared cache (shared.map): only its header and pages are read."""

    def __init__(self, path):
        import mmap
        self.path = path
        self.f = open(path, "rb")
        self.m = mmap.mmap(self.f.fileno(), 0, access=mmap.ACCESS_READ)

    def u32(self, o):
        return struct.unpack_from("<I", self.m, o)[0]


def _shared(path):
    if path not in _SHARED:
        _SHARED[path] = _Raw(path)
    return _SHARED[path]
