# Halo 2 lightmaps and sky fog (docs/format/halo2.md 7 and 10). Read-only.
# Usage: python probe_h2_lightmap.py [map]   (default lockout)
import struct, sys
from h2_common import Map

m = Map(sys.argv[1] if len(sys.argv) > 1 else "lockout")
scnr = m.find("scnr")
sm, so = m.meta(scnr["id"])

# Sky fog: sky +0x48, 0x18 each: colour, max density, start, opaque distance.
for s in sm.elements(so + 0x8, 8):
    km, ko = m.meta(sm.u32(s + 4))
    for f in km.elements(ko + 0x48, 0x18):
        print("sky fog colour %s density %.2f from %.1f to %.1f wu" % (
            tuple(round(c, 3) for c in km.vec(f)), km.f32(f + 0xC), km.f32(f + 0x10), km.f32(f + 0x14)))

for b in sm.elements(so + 0x210, 0x44):
    lm, lo = m.meta(sm.u32(b + 0x18 + 4))
    bm, bo = m.meta(sm.u32(b + 0x10 + 4))
    for g in lm.elements(lo + 0x80, 0x84):
        print("lightmap bitmap", lm.tag_name(lm.u32(g + 0x18 + 4)))
        clusters = [lm.i16(r) for r in lm.elements(g + 0x28, 4)]
        instances = [lm.i16(r) for r in lm.elements(g + 0x48, 4)]
        print("cluster pages ", clusters)
        print("instance pages", instances)

    # Vertex buffer 3 of each cluster section: two floats per vertex, 0..1.
    def buffers(s):
        nv, ptr, size, hdr = bm.u16(s), bm.u32(s + 0x28), bm.u32(s + 0x2C), bm.u32(s + 0x30)
        raw = bm.d[ptr & 0x3FFFFFFF:(ptr & 0x3FFFFFFF) + size]
        out = {}
        for r in bm.elements(s + 0x38, 16):
            if bm.u8(r) == 2:
                o = hdr + 8 + bm.u32(r + 12)
                out[bm.i16(r + 6)] = raw[o:o + bm.u32(r + 8)]
        return nv, out
    for k, c in enumerate(bm.elements(bo + 0x9C, 0xB0)):
        nv, bufs = buffers(c)
        if 3 in bufs and nv:
            uv = [struct.unpack_from("<2f", bufs[3], 8 * i) for i in range(nv)]
            print("cluster %2d: %4d vertices, buffer 3 %d bytes each, u %.3f..%.3f v %.3f..%.3f" % (
                k, nv, len(bufs[3]) // nv, min(a for a, _ in uv), max(a for a, _ in uv),
                min(b for _, b in uv), max(b for _, b in uv)))
