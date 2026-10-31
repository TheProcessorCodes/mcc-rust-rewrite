# Read-only resource access for the MCC Halo 3 (U13) probes: from a tag's
# resource id to the entry, its definition (fixups applied) and the bytes of
# its segment, following docs/format/resources.md section 8. Never writes.
# Use:  from h3_resources import Resources;  R = Resources(m)  (m = h3_common.Map)
import mmap
import os
import struct
import zlib



class Resources:
    def __init__(self, m):
        self.m = m
        zone = m.meta(m.find("zone"))
        play_i = m.find("play")
        play = m.meta(play_i) if play_i is not None and m.tags[play_i][1] != 0 else zone + 0x1C
        self.entries = m.block(zone + 0x64)
        self.fixup_base = m.ptr32(m.u32(zone + 0x154))
        self.fixup_size = m.i32(zone + 0x148)
        self.type_names = [m.sid_at(o + 24) for o in m.elements(zone + 4, 28)]
        self.shared = [m.cstr(o, 256) for o in m.elements(play + 0xC, 264)]
        self.pages = [(m.i16(o + 4), m.i32(o + 8), m.i32(o + 12), m.i32(o + 16))
                      for o in m.elements(play + 0x18, 88)]
        self.segments = [(m.i16(o), m.i16(o + 2), m.i32(o + 4), m.i32(o + 8))
                         for o in m.elements(play + 0x3C, 16)]
        self.page_cache = {}
        self.files = {}

    def entry(self, rid):
        m = self.m
        idx = rid & 0xFFFF
        o = self.entries[1] + 64 * idx
        e = {
            "index": idx,
            "owner": m.ref(o),
            "salt": m.u16(o + 16),
            "type": m.i8(o + 18),
            "fixup_off": m.i32(o + 20),
            "fixup_len": m.i32(o + 24),
            "location": m.i16(o + 32),
            "segment": m.i16(o + 34),
            "def_addr": m.u32(o + 36),
            "fixups": [(m.u32(f), m.u32(f + 4)) for f in m.elements(o + 40, 8)],
            "def_fixups": [(m.u32(f), m.i32(f + 4)) for f in m.elements(o + 52, 8)],
        }
        e["type_name"] = self.type_names[e["type"]] if 0 <= e["type"] < len(self.type_names) else None
        e["salt_ok"] = e["salt"] == (rid >> 16)
        return e

    def definition(self, e, fixed=True):
        start = self.fixup_base + e["fixup_off"]
        d = bytearray(self.m.m[start:start + e["fixup_len"]])
        if fixed:
            for at, value in e["fixups"]:
                if at + 4 <= len(d):
                    d[at:at + 4] = struct.pack("<I", value)
        return bytes(d)

    def _file(self, cache_index):
        if cache_index < 0:
            return self.m.m
        if cache_index not in self.files:
            name = self.shared[cache_index].split("\\")[-1]
            path = os.path.join(os.path.dirname(self.m.path), name)
            with open(path, "rb") as f:
                self.files[cache_index] = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
        return self.files[cache_index]

    def page(self, pi):
        if pi in self.page_cache:
            return self.page_cache[pi]
        cache_index, off, csize, usize = self.pages[pi]
        f = self._file(cache_index)
        base = struct.unpack_from("<I", f, 0x4D0)[0]
        raw = f[base + off:base + off + csize]
        data = raw if csize == usize else zlib.decompressobj(-15).decompress(raw)
        assert len(data) == usize, (pi, len(data), usize)
        self.page_cache[pi] = data
        return data

    def data(self, e, secondary=False):
        """(page bytes, segment start) for the entry's primary (or secondary) page."""
        seg = self.segments[e["segment"]]
        pi, off = (seg[1], seg[3]) if secondary else (seg[0], seg[2])
        if pi < 0 or off < 0:
            return None, None
        return self.page(pi), off
