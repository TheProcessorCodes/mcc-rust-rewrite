# Shared helpers for the MCC Halo 2 probes (docs/format/halo2.md). Read-only:
# the .map is inflated in memory (section 1), nothing is written next to it.
import os, struct, zlib

MAPS = r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo2\h2_maps_win64_dx11"


def inflate(raw):
    """Section 1: header kept as is, then the chunk table's zlib streams."""
    i32 = lambda o: struct.unpack_from("<i", raw, o)[0]
    if struct.unpack_from("<H", raw, 0x1C)[0] & 1 == 0:
        return raw
    table, count = i32(0x310), i32(0x314)
    out = bytearray(raw[:0x380])
    for k in range(count):
        size, addr = i32(table + 8 * k), i32(table + 8 * k + 4)
        if size < 0:
            out += raw[addr + 2:addr + 2 - size]
        else:
            out += zlib.decompressobj().decompress(raw[addr:addr + size])
    return bytes(out)


class Map:
    def __init__(self, name):
        self.path = name if name.lower().endswith(".map") else os.path.join(MAPS, name + ".map")
        self.d = inflate(open(self.path, "rb").read())
        d = self.d
        self.index = self.u32(0x10)
        self.mask = self.u32(0x2D0)
        ia = self.index
        self.class_count, self.tag_count = self.i32(ia + 4), self.i32(ia + 24)
        cls, tf = ia + self.u32(ia), ia + self.u32(ia + 8)
        self.classes = [(d[cls + 12 * k:cls + 12 * k + 4][::-1].decode("latin1"),) for k in range(self.class_count)]
        names, nidx = self.u32(0x24), self.u32(0x2C)
        self.tags = []
        for k in range(self.tag_count):
            o = tf + 16 * k
            c = d[o:o + 4][::-1].decode("latin1")
            tid, mp, size = self.u32(o + 4), self.u32(o + 8), self.u32(o + 12)
            n = self.cstr(names + self.i32(nidx + 4 * k)) if tid != 0xFFFFFFFF else ""
            self.tags.append(dict(index=k, cls=c, id=tid, meta=mp, size=size, name=n))
        sidx, stab, scount = self.u32(0x3C), self.u32(0x34), self.i32(0x30)
        self.strings = [self.cstr(stab + self.i32(sidx + 4 * k)) for k in range(scount)]
        self._shared = None

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
        """Section 2: a tag data pointer to an offset in the inflated file."""
        return p - self.mask + self.index if p else None

    def block(self, o):
        """(count, file offset) of the tag block at o."""
        return self.i32(o), self.addr(self.u32(o + 4))

    def elements(self, o, size):
        n, a = self.block(o)
        return [a + size * k for k in range(n)] if n > 0 and a else []

    def string(self, sid):
        """Section 3: low 24 bits index the string table."""
        if sid == 0:
            return ""
        k = sid & 0xFFFFFF
        return self.strings[k] if k < len(self.strings) else "<sid %x>" % sid

    def tag_by_id(self, tid):
        if tid == 0xFFFFFFFF:
            return None
        t = self.tags[tid & 0xFFFF]
        return t if t["id"] == tid else None

    def tag_name(self, tid):
        t = self.tag_by_id(tid)
        return "%s:%s" % (t["cls"], t["name"]) if t else "<none %x>" % tid

    def ref(self, o):
        """An 8-byte tag reference: class, then datum."""
        return self.u32(o + 4)

    def find(self, cls, name=None):
        for t in self.tags:
            if t["cls"] == cls and (name is None or t["name"] == name) and t["id"] != 0xFFFFFFFF:
                return t
        return None

    def shared(self):
        if self._shared is None:
            self._shared = Map(os.path.join(os.path.dirname(self.path), "shared.map"))
        return self._shared

    def meta(self, tid):
        """(map holding the tag, offset of its data); shared tags live in shared.map."""
        t = self.tag_by_id(tid)
        if t is None:
            return None, None
        if t["meta"] == 0:
            s = self.shared()
            st = s.tag_by_id(tid)
            return (s, s.addr(st["meta"])) if st and st["meta"] else (None, None)
        return self, self.addr(t["meta"])
