# Read-only probe of MCC Halo 2 bitmaps (docs/format/halo2.md 9): every bitmap
# tag a map references, its first image's size and format, and whether its
# pixels inflate from textures.dat to the expected byte count (mip 0 first,
# then the smaller levels).
# Run: python -I probe_h2_bitmaps.py [map] [name filter]
import sys, os, struct, zlib, collections
sys.path.insert(0, os.path.dirname(__file__))
from h2_common import Map

m = Map(sys.argv[1] if len(sys.argv) > 1 else "coagulation")
flt = sys.argv[2] if len(sys.argv) > 2 else ""
tex = open(os.path.join(os.path.dirname(m.path), "textures.dat"), "rb")
FORMATS = {0: ("A8", 8), 1: ("Y8", 8), 2: ("AY8", 8), 3: ("A8Y8", 16), 6: ("R5G6B5", 16), 8: ("A1R5G5B5", 16),
           9: ("A4R4G4B4", 16), 10: ("X8R8G8B8", 32), 11: ("A8R8G8B8", 32), 14: ("DXT1", 4), 15: ("DXT3", 8),
           16: ("DXT5", 8), 17: ("P8_bump", 8), 18: ("P8", 8), 22: ("U8V8", 16)}


def chain_size(fmt, w, h, mips, faces):
    name, bpp = FORMATS.get(fmt, ("?", 0))
    total = 0
    for l in range(mips + 1):
        lw, lh = max(1, w >> l), max(1, h >> l)
        if name.startswith("DXT"):
            total += ((lw + 3) // 4) * ((lh + 3) // 4) * (8 if name == "DXT1" else 16)
        else:
            total += lw * lh * bpp // 8
    return total * faces


def read_pixels(ptr):
    """textures.dat at ptr & 0x3FFFFFFF: int32 n, n int32 chunk sizes
    (negative: stored), then the chunks back to back."""
    tex.seek(ptr & 0x3FFFFFFF)
    n = struct.unpack("<i", tex.read(4))[0]
    sizes = struct.unpack("<%di" % n, tex.read(4 * n))
    out = bytearray()
    for s in sizes:
        raw = tex.read(abs(s))
        out += raw if s < 0 else zlib.decompress(raw)
    return bytes(out), sizes


stats = collections.Counter()
for t in m.tags:
    if t["cls"] != "bitm" or t["id"] == 0xFFFFFFFF or flt not in t["name"]:
        continue
    mm, a = m.meta(t["id"])
    if a is None:
        continue
    for k, b in enumerate(mm.elements(a + 0x44, 0xA8)):
        w, h, depth = mm.i16(b + 4), mm.i16(b + 6), mm.u8(b + 8)
        ty, fmt, flags, mips = mm.u16(b + 0xA), mm.u16(b + 0xC), mm.u16(b + 0xE), mm.i16(b + 0x14)
        ptr, size = mm.u32(b + 0x38), mm.i32(b + 0x50)
        faces = 6 if ty == 2 else 1
        expect = chain_size(fmt, w, h, mips, faces)
        try:
            data, chunks = read_pixels(ptr)
            got = len(data)
        except Exception as e:
            got, chunks = -1, str(e)
        ok = "ok" if got == size else "SIZE"
        stats[(FORMATS.get(fmt, ("?",))[0], ty, ok, got >= expect)] += 1
        if k == 0 or flt:
            print("%-70s %d: %dx%d d%d type %d %s flags %x mips %d ptr %x lod0 %d inflated %d chain %d %s" % (
                t["name"][-70:], k, w, h, depth, ty, FORMATS.get(fmt, (fmt,))[0], flags, mips, ptr, size, got, expect, ok))
print(dict(stats))
