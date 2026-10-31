# Read-only probe of water in MCC Halo 3 (U13) maps: the sbsp's tag blocks and
# anything that references a water (`rmw `) shader. Companion to docs/format/water.md.
# Run: python -I probe_water.py [map name or path] [--blocks] [--groups]
import struct, sys, os

MAPS = os.environ.get("HALO3_MAPS", "")
args = list(sys.argv[1:])
flags = [a for a in args if a.startswith("--")]
args = [a for a in args if not a.startswith("--")]
name = args[0] if args else "riverworld"
path = name if name.lower().endswith(".map") else os.path.join(MAPS, name + ".map")
d = open(path, "rb").read()
print("file", path, "size 0x%X" % len(d))

def i32(o): return struct.unpack_from("<i", d, o)[0]
def u32(o): return struct.unpack_from("<I", d, o)[0]
def i16(o): return struct.unpack_from("<h", d, o)[0]
def u16(o): return struct.unpack_from("<H", d, o)[0]
def i64(o): return struct.unpack_from("<q", d, o)[0]
def f32(o): return struct.unpack_from("<f", d, o)[0]
def cstr(o, n=256):
    e = d.index(b"\0", o); return d[o:min(e, o + n)].decode("latin1")

secoff = [u32(0x4CC + 4 * i) for i in range(4)]
secs = [(u32(0x4DC + 8 * i), u32(0x4E0 + 8 * i)) for i in range(4)]
vbase = i64(0x2E0); idxptr = i64(0x2E8)
tagcount = i32(0x20)
magic2 = (secs[2][0] + secoff[2]) & 0xFFFFFFFF
def tagfile(v): return v - vbase + magic2
def hdr(p, sec=0): return (p + secoff[sec]) & 0xFFFFFFFF
def tp(p): return tagfile((p << 2) & 0xFFFFFFFFFFFFFFFF) if p != 0 else -1

ti = tagfile(idxptr)
classcount = i32(ti); classptr = i64(ti + 8); tagptr = i64(ti + 24)
cf = tagfile(classptr); tf = tagfile(tagptr)
classes = [d[cf + 16 * i:cf + 16 * i + 4][::-1].decode("latin1") for i in range(classcount)]
nameidx = hdr(i32(0x2C)); names = hdr(i32(0x24))
tags = []
for i in range(tagcount):
    ci = i16(tf + 8 * i); mp = u32(tf + 8 * i + 4)
    no = i32(nameidx + 4 * i)
    nm = cstr(names + no) if no >= 0 else None
    tags.append((classes[ci] if ci >= 0 else None, nm, tp(mp) if ci >= 0 else -1))

def block(o):
    c = i32(o); p = u32(o + 4); return c, (tp(p) if c > 0 else -1)

water_tags = [i for i, t in enumerate(tags) if t[0] == "rmw "]
print("rmw tags:", [(i, tags[i][1]) for i in water_tags])
sbsps = [i for i, t in enumerate(tags) if t[0] == "sbsp"]

for s in sbsps:
    m = tags[s][2]
    print("\n=== sbsp", s, tags[s][1], "meta 0x%X" % m)
    if "--blocks" in flags:
        for o in range(0, 0x3A0, 4):
            c, p = block(m + o)
            if 0 < c < 200000 and 0 < p < len(d) and (u32(m + o + 8) == 0):
                print("  block +0x%03X count %6d file 0x%X  first 16: %s" % (o, c, p, d[p:p + 16].hex(" ")))
    # Any tag reference (class 'rmw ' reversed + tag id) inside the sbsp's blocks.
    for o in range(0, 0x3A0, 4):
        c, p = block(m + o)
        if not (0 < c < 5000 and 0 < p < len(d)):
            continue
        # look for a water shader tag reference within the first 4 KiB of the block
        span = d[p:p + 4096]
        k = span.find(b" wmr")
        if k >= 0:
            tid = i32(p + k + 12)
            print("  rmw ref in block +0x%03X (count %d) at element byte %d: tag %d %s" %
                  (o, c, k, tid & 0xFFFF, tags[tid & 0xFFFF][1] if tid != -1 else None))
