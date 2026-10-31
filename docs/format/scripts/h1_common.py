# Shared helpers for the MCC Halo CE probes (docs/format/halo1.md). Read-only:
# the .map is read into memory, nothing is written next to it.
import os, struct

MAPS = r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo1\maps"

# Section 2: tag data is loaded at this address; pointers inside it are
# addresses, made file offsets by subtracting it and adding the tag data
# offset from the header.
BASE = 0x50000000


class Map:
    def __init__(self, name):
        self.path = name if name.lower().endswith(".map") else os.path.join(MAPS, name + ".map")
        self.d = open(self.path, "rb").read()
        assert self.d[:4] == b"daeh" and self.u32(4) == 13, "not an MCC Halo CE map"
        self.tag_data = self.u32(0x10)
        self.tag_data_size = self.u32(0x14)
        self.name = self.cstr(0x20)
        self.build = self.cstr(0x40)
        self.kind = self.u32(0x60)
        t = self.tag_data
        self.scenario_id = self.u32(t + 4)
        self.tag_count = self.u32(t + 0xC)
        self.model_data = self.u32(t + 0x14)
        self.vertex_size = self.u32(t + 0x1C)
        self.model_size = self.u32(t + 0x20)
        arr = self.addr(self.u32(t))
        self.tags = []
        for k in range(self.tag_count):
            o = arr + 0x20 * k
            c = self.d[o:o + 4][::-1].decode("latin1")
            tid, np, dp, ext = self.u32(o + 12), self.u32(o + 16), self.u32(o + 20), self.u32(o + 24)
            self.tags.append(dict(index=k, cls=c, id=tid, data=dp, external=ext, name=self.cstr(self.addr(np))))
        self._bitmaps = None

    def i8(self, o): return struct.unpack_from("<b", self.d, o)[0]
    def u8(self, o): return self.d[o]
    def i16(self, o): return struct.unpack_from("<h", self.d, o)[0]
    def u16(self, o): return struct.unpack_from("<H", self.d, o)[0]
    def i32(self, o): return struct.unpack_from("<i", self.d, o)[0]
    def u32(self, o): return struct.unpack_from("<I", self.d, o)[0]
    def f32(self, o): return struct.unpack_from("<f", self.d, o)[0]
    def vec(self, o, n=3): return struct.unpack_from("<%df" % n, self.d, o)
    def cstr(self, o):
        return self.d[o:self.d.index(b"\0", o)].decode("latin1")

    def addr(self, p):
        """A tag data address to a file offset."""
        return p - BASE + self.tag_data if p else None

    def tag(self, tid):
        if tid == 0xFFFFFFFF:
            return None
        t = self.tags[tid & 0xFFFF]
        return t if t["id"] == tid else None

    def data(self, tid):
        t = self.tag(tid)
        return self.addr(t["data"]) if t and t["data"] else None

    def find(self, cls, name=None):
        return [t for t in self.tags if t["cls"] == cls and (name is None or t["name"].lower() == name.lower())]

    def block(self, o, size):
        """A 12-byte block: count, address, (unused): element offsets."""
        n, p = self.u32(o), self.u32(o + 4)
        if n == 0:
            return []
        a = self.addr(p)
        return [a + size * k for k in range(n)]

    def ref(self, o):
        """A 16-byte tag reference: class, name address, name length, datum."""
        return self.u32(o + 12)

    def refname(self, o):
        t = self.tag(self.ref(o))
        return t["name"] if t else None

    def bsps(self):
        """Section 5: the scenario's structure BSPs (0x20 each at +0x5A4):
        file offset, size, load address, then the sbsp reference."""
        scnr = self.data(self.scenario_id)
        out = []
        for e in self.block(scnr + 0x5A4, 0x20):
            start, size, base = self.u32(e), self.u32(e + 4), self.u32(e + 8)
            out.append(BspFile(self, start, size, base, self.ref(e + 0x10)))
        return out


class BspFile:
    """A structure BSP's own chunk of the file; pointers in it are relative
    to `base`."""
    def __init__(self, m, start, size, base, tid):
        self.m, self.start, self.size, self.base, self.id = m, start, size, base, tid
        self.sbsp = self.addr(m.u32(start))

    def addr(self, p):
        return p - self.base + self.start if p else None

    def block(self, o, size):
        n, p = self.m.u32(o), self.m.u32(o + 4)
        if n == 0:
            return []
        a = self.addr(p)
        return [a + size * k for k in range(n)]
