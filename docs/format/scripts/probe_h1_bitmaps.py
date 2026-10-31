# Bitmap tags and where their pixels live (docs/format/halo1.md 8).
# python -I probe_h1_bitmaps.py [map]
import os, struct, sys
from collections import Counter
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from h1_common import Map, MAPS

m = Map(sys.argv[1] if len(sys.argv) > 1 else "bloodgulch")
bm = open(os.path.join(MAPS, "bitmaps.map"), "rb").read()
print("bitmaps.map header", [hex(x) for x in struct.unpack_from("<4I", bm, 0)], "size", hex(len(bm)))
flags, formats = Counter(), Counter()
past = 0
for t in m.find("bitm"):
    o = m.data(t["id"])
    images = m.block(o + 0x60, 0x30)
    for i in images:
        sig = m.d[i:i + 4][::-1]
        w, h, dp, ty, fmt, fl = struct.unpack_from("<6H", m.d, i + 4)
        mips = m.u16(i + 0x14)
        off, size = m.u32(i + 0x18), m.u32(i + 0x1C)
        flags[hex(fl)] += 1
        formats[(fmt, ty)] += 1
        if off + size > len(bm):
            past += 1
    if t["index"] < 60 and images:
        i = images[0]
        print(t["name"], "type/format/usage/flags", struct.unpack_from("<4H", m.d, o), len(images), "images;",
              sig, w, h, dp, "type", ty, "fmt", fmt, "flags", hex(fl), "mips", mips, "at", hex(off), size,
              [hex(m.u32(i + k)) for k in (0x20, 0x24, 0x28, 0x2C)])
print("image flags", flags)
print("(format, type)", formats)
print("images past the end of bitmaps.map:", past)
