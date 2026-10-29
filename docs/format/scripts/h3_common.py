# Shared read-only helpers for the MCC Halo 3 (U13) probes: opens a .map,
# parses the header pointers, the tag index, tag names and string ids, and
# reads tag references and tag blocks. Never writes anything.
# Use:  sys.path.insert(0, os.path.dirname(__file__)); from h3_common import Map
import mmap
import os
import struct

MAPS = r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo3\maps"


class Map:
    def __init__(self, name_or_path):
        path = name_or_path
        if not os.path.exists(path):
            path = os.path.join(MAPS, name_or_path + ".map")
        self.path = path
        self.f = open(path, "rb")
        m = self.m = mmap.mmap(self.f.fileno(), 0, access=mmap.ACCESS_READ)
        assert m[0:4] == b"daeh"
        tag_count = self.i32(0x20)
        self.sec_off = [self.u32(0x4CC + 4 * i) for i in range(4)]
        sections = [(self.u32(0x4DC + 8 * i), self.u32(0x4DC + 8 * i + 4)) for i in range(4)]
        virtual_base, tag_index_vptr = self.u64(0x2E0), self.u64(0x2E8)
        self.tag_magic = virtual_base - ((sections[2][0] + self.sec_off[2]) & 0xFFFFFFFF)
        ti = self.vaddr(tag_index_vptr)
        class_tbl = self.vaddr(self.u64(ti + 8))
        tag_tbl = self.vaddr(self.u64(ti + 24))
        self.classes = [self.fourcc(class_tbl + 16 * i) for i in range(self.i32(ti))]
        self.tags = [(self.i16(tag_tbl + 8 * i), self.i32(tag_tbl + 8 * i + 4)) for i in range(tag_count)]
        nidx, ntbl = self.hdr(self.u32(0x2C)), self.hdr(self.u32(0x24))
        self.names = [self.cstr(ntbl + self.i32(nidx + 4 * i)) for i in range(tag_count)]
        # String ids: 19 index bits, 8 namespace bits (scenario doc 1.7).
        sc, sidx, stbl = self.i32(0x30), self.hdr(self.u32(0x3C)), self.hdr(self.u32(0x34))
        self.strings = [self.cstr(stbl + o) if o >= 0 else None
                        for o in (self.i32(sidx + 4 * i) for i in range(sc))]
        ns_count, ns_tbl = self.i32(0x40), self.hdr(self.u32(0x44))
        mask = (1 << 19) - 1
        ns = [self.i32(ns_tbl + 4 * i) & mask for i in range(ns_count)]
        self.ns = {}
        start = ns[0]
        for i in range(1, ns_count):
            self.ns[i] = (0, start)
            start += ns[i]
        self.ns[0] = (ns[0], start)

    # primitives
    def i8(self, o): return struct.unpack_from("<b", self.m, o)[0]
    def u8(self, o): return self.m[o]
    def i16(self, o): return struct.unpack_from("<h", self.m, o)[0]
    def u16(self, o): return struct.unpack_from("<H", self.m, o)[0]
    def i32(self, o): return struct.unpack_from("<i", self.m, o)[0]
    def u32(self, o): return struct.unpack_from("<I", self.m, o)[0]
    def u64(self, o): return struct.unpack_from("<Q", self.m, o)[0]
    def f32(self, o): return struct.unpack_from("<f", self.m, o)[0]
    def f32n(self, o, n): return struct.unpack_from("<" + "f" * n, self.m, o)

    def cstr(self, o, maxlen=256):
        end = self.m.find(b"\0", o, o + maxlen)
        return self.m[o:end if end >= 0 else o + maxlen].decode("latin-1")

    def fourcc(self, o): return self.m[o:o + 4][::-1].decode("latin-1")
    def hdr(self, p): return (p + self.sec_off[0]) & 0xFFFFFFFF
    def vaddr(self, v): return v - self.tag_magic
    def ptr32(self, p): return self.vaddr((p & 0xFFFFFFFF) << 2)

    # tags
    def cls(self, i):
        c = self.tags[i][0]
        return self.classes[c] if c >= 0 else None

    def meta(self, i): return self.ptr32(self.tags[i][1])

    def find(self, cls, name=None):
        for i in range(len(self.tags)):
            if self.cls(i) == cls and (name is None or self.names[i] == name):
                return i
        return None

    def all(self, cls): return [i for i in range(len(self.tags)) if self.cls(i) == cls]

    def ref(self, o):
        """16-byte tag reference -> tag index or None."""
        idx = struct.unpack("<h", struct.pack("<H", self.u32(o + 12) & 0xFFFF))[0]
        return idx if idx >= 0 else None

    def refname(self, o):
        i = self.ref(o)
        return f"{self.cls(i)} {self.names[i]}" if i is not None else "-"

    def block(self, o):
        """12-byte tag block -> (count, file offset of element 0 or None)."""
        n = self.i32(o)
        return n, (self.ptr32(self.u32(o + 4)) if n > 0 else None)

    def elements(self, o, stride):
        n, a = self.block(o)
        return [a + stride * k for k in range(n)] if a is not None else []

    def sid(self, v):
        if v == 0:
            return ""
        mask = (1 << 19) - 1
        idx, ns = v & mask, (v >> 19) & 0xFF
        while ns not in self.ns and ns > 0:
            ns -= 1
        mn, st = self.ns[ns]
        real = idx if idx < mn else idx - mn + st
        return self.strings[real] if 0 <= real < len(self.strings) else f"<sid 0x{v:08X}>"

    def sid_at(self, o): return self.sid(self.u32(o))

    def scan(self, base, size):
        """Every plausible tag reference and tag block in [base, base + size)."""
        out = []
        for off in range(0, size, 4):
            o = base + off
            c = self.u32(o)
            tid = self.u32(o + 12) if o + 16 <= len(self.m) else 0
            idx = tid & 0xFFFF
            if c != 0xFFFFFFFF and idx < len(self.tags) and self.cls(idx) == self.fourcc(o) and (tid >> 16) != 0:
                out.append((off, "ref", f"{self.cls(idx)} {self.names[idx]}"))
                continue
            n, p = self.i32(o), self.u32(o + 4)
            if 0 < n < 5000 and p:
                fo = self.ptr32(p)
                if 0 < fo < len(self.m):
                    out.append((off, "block", f"count {n} at 0x{fo:X}"))
        return out
